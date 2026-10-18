//! One stable exclusive lease and append-only native-handle segments. Every intent/result frame is flushed
//! before it is published. Torn final frames are preserved, never truncated; a continuation binds the exact
//! preceding segment identity, all its bytes, and the last committed sequence before any later event.
//!
//! These are process-crash consistency guarantees. File flushes do not make a journal and a namespace rename
//! one power-loss transaction, nor certify persistence of newly created directory entries/hardware caches.
use super::{Error, names::Name};
use crate::exec::{
    Fail,
    sys::{Parent, reorg as native},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider, BCryptHash,
    BCryptOpenAlgorithmProvider,
};

const MAGIC: &[u8; 4] = b"VJR2";
const HEADER: usize = 48;
const COMMIT: &[u8; 8] = b"COMMIT1!";
const MAX_RECORD: usize = 1024 * 1024;
const MAX_TOTAL: usize = 32 * 1024 * 1024;
const MAX_SEGMENTS: usize = 32;
const LEASE: &[u8] = b"VACANCY-REORG-LEASE-1\n";

/// Bound serialized work without first allocating an oversized JSON buffer.
pub(super) fn check_record(event: &impl Serialize) -> Result<(), Error> {
    struct Limit(usize);
    impl Write for Limit {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_RECORD - self.0 {
                return Err(std::io::Error::other("record bound"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Limit(0), event).map_err(|_| Error::Bounds)
}

#[derive(Debug)]
struct Hash(BCRYPT_ALG_HANDLE);
impl Hash {
    fn new() -> Result<Self, Error> {
        let mut handle = ptr::null_mut();
        // SAFETY: handle is writable; algorithm is a static terminated string; default provider, no flags.
        let result = unsafe {
            BCryptOpenAlgorithmProvider(&mut handle, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0)
        };
        if result < 0 {
            return Err(Error::JournalUnavailable(
                "SHA-256 provider unavailable".into(),
            ));
        }
        Ok(Self(handle))
    }
    fn of(&self, bytes: &[u8]) -> Result<[u8; 32], Error> {
        let mut out = [0; 32];
        let length = u32::try_from(bytes.len()).map_err(|_| Error::Bounds)?;
        // SAFETY: provider is owned/live; both bounded buffers have the stated lengths. No secret/auth token.
        let result = unsafe {
            BCryptHash(
                self.0,
                ptr::null(),
                0,
                bytes.as_ptr(),
                length,
                out.as_mut_ptr(),
                32,
            )
        };
        if result < 0 {
            return Err(Error::JournalUnavailable("SHA-256 failed".into()));
        }
        Ok(out)
    }
}
impl Drop for Hash {
    fn drop(&mut self) {
        // SAFETY: this provider is owned here and no hash operation outlives the store.
        unsafe {
            BCryptCloseAlgorithmProvider(self.0, 0);
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Continuation {
    previous: Vec<Binding>,
    last_sequence: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    identity: native::Id,
    digest: [u8; 32],
}

struct Segment {
    file: File,
    id: native::Id,
    bytes: Vec<u8>,
    torn: bool,
}

pub(super) struct Store {
    parent: Parent,
    name: Name,
    _lease: File,
    lease_identity: native::Id,
    segments: Vec<Segment>,
    hash: Hash,
    sequence: u64,
    poisoned: bool,
    records: Vec<Value>,
    pub(super) flushes: u64,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("segments", &self.segments.len())
            .field("sequence", &self.sequence)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

fn io(error: std::io::Error) -> Error {
    Error::JournalUnavailable(error.to_string())
}
fn native_error(error: Fail) -> Error {
    match error {
        Fail::InUse(_) => Error::Busy,
        _ => Error::JournalUnavailable(format!("{error:?}")),
    }
}
fn read(file: &mut File, remaining: usize) -> Result<Vec<u8>, Error> {
    file.seek(SeekFrom::Start(0)).map_err(io)?;
    let mut bytes = Vec::new();
    file.take((remaining + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > remaining {
        return Err(Error::Bounds);
    }
    Ok(bytes)
}

impl Store {
    /// The caller supplies a trusted app-data location. All actual child opens use this retained native parent.
    pub(super) fn open(
        path: &Path,
        new: bool,
        expected_parent: &[native::Id],
    ) -> Result<Self, Error> {
        let name = Name::checked(
            path.file_name()
                .ok_or(Error::InvalidName)?
                .encode_wide()
                .collect(),
        )?;
        if name.0.len() > 220 {
            return Err(Error::InvalidName);
        }
        let parent =
            native::directory(path.parent().ok_or(Error::InvalidName)?).map_err(native_error)?;
        if parent
            .steps
            .iter()
            .map(native::Id::step)
            .collect::<Vec<_>>()
            != expected_parent
        {
            return Err(Error::Changed);
        }
        super::windows::boundary("journal-parent");
        let lease_name = suffix(&name, ".lease")?;
        let (mut lease, lease_identity) =
            native::journal_file(&parent.dir, &lease_name, native::JournalOpen::Lease)
                .map_err(native_error)?;
        let content = read(&mut lease, 64)?;
        if content.is_empty() {
            lease.write_all(LEASE).map_err(io)?;
            lease.sync_all().map_err(io)?;
        } else if content != LEASE {
            return Err(Error::CorruptJournal);
        }
        let hash = Hash::new()?;
        let mut segments: Vec<Segment> = Vec::new();
        let mut records = Vec::new();
        let mut sequence = 0;
        let mut total = 0;
        let mut gap = false;
        for index in 0..MAX_SEGMENTS {
            let child = segment_name(&name, index)?;
            let opened = native::journal_file(&parent.dir, &child, native::JournalOpen::Existing);
            let (mut file, id) = match opened {
                Ok(value) if !gap && !new => value,
                Ok(_) => return Err(Error::CorruptJournal),
                Err(Fail::Gone) => {
                    gap = true;
                    continue;
                }
                Err(error) => return Err(native_error(error)),
            };
            let bytes = read(&mut file, MAX_TOTAL - total)?;
            total += bytes.len();
            let base_sequence = sequence;
            let (mut decoded, last_sequence, torn) = decode(&bytes, sequence, &hash)?;
            if index > 0 && !decoded.is_empty() {
                let first = decoded.remove(0);
                let binding: Continuation =
                    serde_json::from_value(first).map_err(|_| Error::CorruptJournal)?;
                if binding.previous.len() != segments.len()
                    || binding.last_sequence != base_sequence
                {
                    return Err(Error::CorruptJournal);
                }
                for (bound, previous) in binding.previous.iter().zip(&segments) {
                    if bound.identity != previous.id || bound.digest != hash.of(&previous.bytes)? {
                        return Err(Error::CorruptJournal);
                    }
                }
            }
            sequence = last_sequence;
            records.extend(decoded);
            segments.push(Segment {
                file,
                id,
                bytes,
                torn,
            });
        }
        if new {
            let (file, id) = native::journal_file(&parent.dir, &name, native::JournalOpen::Create)
                .map_err(native_error)?;
            segments.push(Segment {
                file,
                id,
                bytes: Vec::new(),
                torn: false,
            });
        } else if records.is_empty() {
            return Err(Error::CorruptJournal);
        }
        Ok(Self {
            parent,
            name,
            _lease: lease,
            lease_identity,
            segments,
            hash,
            sequence,
            poisoned: false,
            records,
            flushes: 0,
        })
    }

    pub(super) fn records(&self) -> &[Value] {
        &self.records
    }

    pub(super) fn append(&mut self, event: &impl Serialize) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::JournalUnavailable("writer previously failed".into()));
        }
        let value = serde_json::to_value(event).map_err(|_| Error::CorruptJournal)?;
        // A new segment is a journal-only continuation, never an automatic user-data action.
        let last = self.segments.last().ok_or(Error::CorruptJournal)?;
        if last.torn || (self.segments.len() > 1 && last.bytes.is_empty()) {
            if self.segments.len() >= MAX_SEGMENTS {
                return Err(Error::Bounds);
            }
            let previous = self
                .segments
                .iter()
                .map(|segment| {
                    Ok(Binding {
                        identity: segment.id,
                        digest: self.hash.of(&segment.bytes)?,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            let continuation = Continuation {
                previous,
                last_sequence: self.sequence,
            };
            let child = segment_name(&self.name, self.segments.len())?;
            let (file, id) =
                native::journal_file(&self.parent.dir, &child, native::JournalOpen::Create)
                    .map_err(native_error)?;
            self.segments.push(Segment {
                file,
                id,
                bytes: Vec::new(),
                torn: false,
            });
            super::windows::boundary("segment-create");
            self.write(&continuation)?;
            super::windows::boundary("segment-bound");
        }
        self.write(&value)?;
        self.records.push(value);
        Ok(())
    }

    fn write(&mut self, event: &impl Serialize) -> Result<(), Error> {
        let payload = serde_json::to_vec(event).map_err(|_| Error::CorruptJournal)?;
        if payload.len() > MAX_RECORD {
            return Err(Error::Bounds);
        }
        let next = self.sequence.checked_add(1).ok_or(Error::Bounds)?;
        let mut frame = Vec::with_capacity(HEADER + 40 + payload.len());
        frame.extend_from_slice(MAGIC);
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&next.to_le_bytes());
        // Check the fixed header before trusting its variable length. A corrupted interior length must
        // never hide later committed receipts by masquerading as an incomplete final record.
        frame.extend_from_slice(&self.hash.of(&frame)?);
        frame.extend(payload);
        frame.extend_from_slice(&self.hash.of(&frame)?);
        frame.extend_from_slice(COMMIT);
        if self.segments.iter().map(|s| s.bytes.len()).sum::<usize>() + frame.len() > MAX_TOTAL {
            return Err(Error::Bounds);
        }
        // An append/flush failure may have left any prefix of a record. Never retry using this writer.
        self.poisoned = true;
        native::revalidate(&self.parent).map_err(native_error)?;
        native::journal_check(&self._lease, self.lease_identity).map_err(native_error)?;
        for segment in &self.segments {
            native::journal_check(&segment.file, segment.id).map_err(native_error)?;
        }
        let last = self.segments.last_mut().ok_or(Error::CorruptJournal)?;
        // Flush access includes WRITE_DATA. This private writer enforces append-only behavior with
        // exclusive ownership and an exact EOF check; it exposes no truncate/overwrite operation.
        if last.file.seek(SeekFrom::End(0)).map_err(io)? != last.bytes.len() as u64 {
            return Err(Error::CorruptJournal);
        }
        #[cfg(test)]
        if let Some(fault) = tests::fault(next) {
            match fault {
                tests::Fault::Before => {
                    return Err(Error::JournalUnavailable("injected before write".into()));
                }
                tests::Fault::Partial(count) => {
                    last.file
                        .write_all(&frame[..count.min(frame.len())])
                        .map_err(io)?;
                    last.file.sync_all().map_err(io)?;
                    return Err(Error::JournalUnavailable("injected torn write".into()));
                }
                tests::Fault::Flush => {
                    last.file.write_all(&frame).map_err(io)?;
                    return Err(Error::JournalUnavailable("injected flush failure".into()));
                }
            }
        }
        last.file.write_all(&frame).map_err(io)?;
        last.file.sync_all().map_err(io)?;
        self.flushes += 1;
        last.bytes.extend(frame);
        last.torn = false;
        self.sequence = next;
        self.poisoned = false;
        Ok(())
    }
}

fn suffix(name: &Name, text: &str) -> Result<Name, Error> {
    Name::checked(name.0.iter().copied().chain(text.encode_utf16()).collect())
}
fn segment_name(name: &Name, index: usize) -> Result<Name, Error> {
    if index == 0 {
        Ok(name.clone())
    } else {
        suffix(name, &format!(".part{index:04}"))
    }
}

/// Only a structurally incomplete final record is a tail. Complete bad frames never become recoverable tails.
fn decode(bytes: &[u8], mut sequence: u64, hash: &Hash) -> Result<(Vec<Value>, u64, bool), Error> {
    let mut at = 0;
    let mut records = Vec::new();
    while at < bytes.len() {
        let remaining = &bytes[at..];
        if remaining[..remaining.len().min(4)] != MAGIC[..remaining.len().min(4)] {
            return Err(Error::CorruptJournal);
        }
        if remaining.len() < 16 {
            return Ok((records, sequence, true));
        }
        let length = u32::from_le_bytes(
            remaining[4..8]
                .try_into()
                .map_err(|_| Error::CorruptJournal)?,
        ) as usize;
        let next = u64::from_le_bytes(
            remaining[8..16]
                .try_into()
                .map_err(|_| Error::CorruptJournal)?,
        );
        if length > MAX_RECORD || next != sequence + 1 {
            return Err(Error::CorruptJournal);
        }
        if remaining.len() < HEADER {
            return Ok((records, sequence, true));
        }
        if remaining[16..HEADER] != hash.of(&remaining[..16])? {
            return Err(Error::CorruptJournal);
        }
        let end = HEADER + length;
        if remaining.len() < end + 40 {
            return Ok((records, sequence, true));
        }
        if remaining[end..end + 32] != hash.of(&remaining[..end])?
            || &remaining[end + 32..end + 40] != COMMIT
        {
            return Err(Error::CorruptJournal);
        }
        records.push(
            serde_json::from_slice(&remaining[HEADER..end]).map_err(|_| Error::CorruptJournal)?,
        );
        sequence = next;
        at += end + 40;
    }
    Ok((records, sequence, false))
}

#[cfg(test)]
pub(super) mod tests {
    use std::cell::RefCell;
    #[derive(Clone, Copy)]
    pub(crate) enum Fault {
        Before,
        Partial(usize),
        Flush,
    }
    thread_local! { static FAULT: RefCell<Option<(u64, Fault)>> = const { RefCell::new(None) }; }
    pub(crate) fn set(sequence: u64, fault: Fault) {
        FAULT.with(|slot| *slot.borrow_mut() = Some((sequence, fault)));
    }
    pub(crate) fn fault(sequence: u64) -> Option<Fault> {
        FAULT.with(|slot| {
            let mut value = slot.borrow_mut();
            if value.as_ref().is_some_and(|(at, _)| *at == sequence) {
                value.take().map(|(_, fault)| fault)
            } else {
                None
            }
        })
    }
}
