//! What the elevated reader sends the app, over the pipe: framed messages, little-endian.
//!
//! A frame is a tag byte, a `u32` payload length (at most [`MAX_FRAME`]), and the payload. The stream is:
//! - [`Header`] first;
//! - [`Progress`] now and then while the index is read;
//! - then the visible folders, parents before children: each as `Dir`, one or more `Entries` frames, and `DirEnd`;
//! - a `Locked` frame for each folder the app's account can't open, with its totals and no names inside it;
//! - `Records`, the bytes NTFS keeps for itself;
//! - and `End`; or, instead of any of it, `Error`.
//!
//! The app reads it as untrusted data: [`Decoder`] only checks the framing and the field bounds here; what the
//! messages say is checked as it is used (see `vacancy-core`'s fast scan).

use std::io::{self, Read, Write};

/// The first bytes of the header.
pub const MAGIC: [u8; 8] = *b"VACIDX01";
/// The protocol's version.
pub const VERSION: u32 = 1;
/// What the app writes once it has connected, before the reader sends anything. Nothing else is ever read.
pub const HELLO: [u8; 16] = *b"VACANCY-HELLO-01";
/// The biggest payload a frame may have.
pub const MAX_FRAME: usize = 1 << 20;
/// The longest name NTFS allows, in UTF-16 units.
pub const MAX_NAME: usize = 255;

const TAG_HEADER: u8 = 1;
const TAG_PROGRESS: u8 = 2;
const TAG_DIR: u8 = 3;
const TAG_ENTRIES: u8 = 4;
const TAG_DIR_END: u8 = 5;
const TAG_LOCKED: u8 = 6;
const TAG_RECORDS: u8 = 7;
const TAG_END: u8 = 8;
const TAG_ERROR: u8 = 9;

/// An entry is a folder.
pub const ENTRY_DIR: u8 = 1;
/// A folder the reader will list (it is neither a link nor locked).
pub const ENTRY_ENTER: u8 = 1 << 1;
/// A folder the app's account can't open: never listed, its totals come in a `Locked` frame.
pub const ENTRY_LOCKED: u8 = 1 << 2;

/// Fixed bytes of an entry before its name.
const ENTRY_FIXED: usize = 8 + 4 + 4 + 8 + 8 + 8 + 8 + 1 + 2;

/// The volume the reader read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// The NTFS volume serial number, as `FILE_ID_INFO` gives it.
    pub volume_serial: u64,
    pub cluster_size: u32,
    pub record_size: u32,
    /// Records in the index (used or not).
    pub records: u64,
    /// The volume's size in clusters.
    pub total_clusters: u64,
}

/// How far reading the index has got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub records_done: u64,
    pub records_total: u64,
    pub files: u64,
    pub folders: u64,
}

/// One name in a folder, as a directory listing would give it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    /// The file id: record number and sequence, as Windows gives it.
    pub frn: u64,
    /// `FILE_ATTRIBUTE_*`, with `DIRECTORY` for folders.
    pub attributes: u32,
    /// The reparse tag, or 0.
    pub tag: u32,
    pub logical: u64,
    pub allocated: u64,
    /// FILETIMEs, as listings give them.
    pub modified: i64,
    pub accessed: i64,
    /// `ENTRY_*`.
    pub flags: u8,
    /// UTF-16 units, little-endian.
    name: &'a [u8],
}

impl Entry<'_> {
    /// The name, as UTF-16 units.
    pub fn name_units(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.name
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
    }

    /// Writes the name into `out` (cleared first).
    pub fn name_into(&self, out: &mut Vec<u16>) {
        out.clear();
        out.extend(self.name_units());
    }
}

/// A folder the app's account can't open, with what is in it. No names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locked {
    /// The folder's file id. Its parent listed it with [`ENTRY_LOCKED`].
    pub frn: u64,
    /// Allocated bytes of the files only found inside locked folders, each file once.
    pub bytes: u64,
    pub files: u64,
    /// Folders, this one included.
    pub folders: u64,
}

/// What the reader sent in all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct End {
    pub dirs: u64,
    pub entries: u64,
    pub locked: u64,
}

/// Why the reader couldn't read the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    /// The volume couldn't be opened.
    OpenVolume,
    /// It isn't NTFS (or is locked by BitLocker, whose boot sector isn't NTFS's).
    NotNtfs,
    /// The index couldn't be read or made sense of.
    Unreadable,
    /// The app's account couldn't be checked against folders' permissions.
    Access,
    /// Anything else.
    Other(u32),
}

impl ErrorCode {
    pub fn code(self) -> u32 {
        match self {
            Self::OpenVolume => 1,
            Self::NotNtfs => 2,
            Self::Unreadable => 3,
            Self::Access => 4,
            Self::Other(n) => n,
        }
    }

    pub fn from_code(n: u32) -> Self {
        match n {
            1 => Self::OpenVolume,
            2 => Self::NotNtfs,
            3 => Self::Unreadable,
            4 => Self::Access,
            n => Self::Other(n),
        }
    }
}

/// One frame.
#[derive(Debug)]
pub enum Frame<'a> {
    Header(Header),
    Progress(Progress),
    Dir { frn: u64 },
    Entries(Entries<'a>),
    DirEnd { frn: u64, count: u64 },
    Locked(Locked),
    Records { bytes: u64 },
    End(End),
    Error(ErrorCode),
}

/// The entries of an `Entries` frame, checked as they are read.
#[derive(Debug)]
pub struct Entries<'a> {
    left: u32,
    rest: &'a [u8],
}

impl<'a> Iterator for Entries<'a> {
    type Item = Result<Entry<'a>, WireError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            return (!self.rest.is_empty()).then_some(Err(WireError::Malformed("trailing bytes")));
        }
        self.left -= 1;
        Some(self.entry())
    }
}

impl<'a> Entries<'a> {
    /// How many entries the frame says it has.
    pub fn count(&self) -> u32 {
        self.left
    }

    fn entry(&mut self) -> Result<Entry<'a>, WireError> {
        let b = self.rest;
        let bad = WireError::Malformed("short entry");
        if b.len() < ENTRY_FIXED {
            return Err(bad);
        }
        let mut c = Cursor { b, at: 0 };
        let frn = c.u64()?;
        let attributes = c.u32()?;
        let tag = c.u32()?;
        let logical = c.u64()?;
        let allocated = c.u64()?;
        let modified = c.i64()?;
        let accessed = c.i64()?;
        let flags = c.u8()?;
        let units = usize::from(c.u16()?);
        if units == 0 || units > MAX_NAME {
            return Err(WireError::Malformed("name length"));
        }
        let name = c.take(units * 2)?;
        self.rest = &b[c.at..];
        Ok(Entry {
            frn,
            attributes,
            tag,
            logical,
            allocated,
            modified,
            accessed,
            flags,
            name,
        })
    }
}

/// Why the stream couldn't be read.
#[derive(Debug)]
pub enum WireError {
    /// The pipe failed or closed.
    Io(io::Error),
    /// A frame is malformed.
    Malformed(&'static str),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "the index stream failed: {e}"),
            Self::Malformed(what) => write!(f, "the index stream is malformed: {what}"),
        }
    }
}

impl std::error::Error for WireError {}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or(WireError::Malformed("short frame"))?;
        let out = &self.b[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("2 bytes"),
        ))
    }
    fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }
    fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }
    fn i64(&mut self) -> Result<i64, WireError> {
        Ok(i64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }
    fn done(&self) -> Result<(), WireError> {
        if self.at == self.b.len() {
            Ok(())
        } else {
            Err(WireError::Malformed("frame longer than its fields"))
        }
    }
}

/// Reads frames from a byte source, one at a time, into one reused buffer.
#[derive(Debug)]
pub struct Decoder<R> {
    source: R,
    buf: Vec<u8>,
}

impl<R: Read> Decoder<R> {
    pub fn new(source: R) -> Self {
        Self {
            source,
            buf: Vec::new(),
        }
    }

    /// The next frame, or `None` at a clean end of the stream (between frames).
    pub fn next_frame(&mut self) -> Result<Option<Frame<'_>>, WireError> {
        let mut head = [0u8; 5];
        match read_full(&mut self.source, &mut head)? {
            0 => return Ok(None),
            5 => {}
            _ => return Err(WireError::Malformed("cut off in a frame header")),
        }
        let tag = head[0];
        let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
        if len > MAX_FRAME {
            return Err(WireError::Malformed("frame too long"));
        }
        self.buf.resize(len, 0);
        if read_full(&mut self.source, &mut self.buf)? != len {
            return Err(WireError::Malformed("cut off in a frame"));
        }
        let mut c = Cursor {
            b: &self.buf,
            at: 0,
        };
        let frame = match tag {
            TAG_HEADER => {
                if c.take(8)? != MAGIC {
                    return Err(WireError::Malformed("not an index stream"));
                }
                if c.u32()? != VERSION {
                    return Err(WireError::Malformed("another version of the index stream"));
                }
                Frame::Header(Header {
                    volume_serial: c.u64()?,
                    cluster_size: c.u32()?,
                    record_size: c.u32()?,
                    records: c.u64()?,
                    total_clusters: c.u64()?,
                })
            }
            TAG_PROGRESS => Frame::Progress(Progress {
                records_done: c.u64()?,
                records_total: c.u64()?,
                files: c.u64()?,
                folders: c.u64()?,
            }),
            TAG_DIR => Frame::Dir { frn: c.u64()? },
            TAG_ENTRIES => {
                let left = c.u32()?;
                // Each entry takes at least its fixed part and one name unit.
                if (left as usize).saturating_mul(ENTRY_FIXED + 2) > len {
                    return Err(WireError::Malformed("more entries than the frame holds"));
                }
                return Ok(Some(Frame::Entries(Entries {
                    left,
                    rest: &self.buf[4..],
                })));
            }
            TAG_DIR_END => Frame::DirEnd {
                frn: c.u64()?,
                count: c.u64()?,
            },
            TAG_LOCKED => Frame::Locked(Locked {
                frn: c.u64()?,
                bytes: c.u64()?,
                files: c.u64()?,
                folders: c.u64()?,
            }),
            TAG_RECORDS => Frame::Records { bytes: c.u64()? },
            TAG_END => Frame::End(End {
                dirs: c.u64()?,
                entries: c.u64()?,
                locked: c.u64()?,
            }),
            TAG_ERROR => Frame::Error(ErrorCode::from_code(c.u32()?)),
            _ => return Err(WireError::Malformed("unknown frame")),
        };
        c.done()?;
        Ok(Some(frame))
    }
}

/// Reads until `buf` is full or the source ends; how many bytes were read.
fn read_full(source: &mut impl Read, buf: &mut [u8]) -> Result<usize, WireError> {
    let mut at = 0;
    while at < buf.len() {
        match source.read(&mut buf[at..]) {
            Ok(0) => break,
            Ok(n) => at += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(WireError::Io(e)),
        }
    }
    Ok(at)
}

/// An entry to send.
#[derive(Clone, Copy, Debug)]
pub struct OutEntry<'a> {
    pub frn: u64,
    pub attributes: u32,
    pub tag: u32,
    pub logical: u64,
    pub allocated: u64,
    pub modified: i64,
    pub accessed: i64,
    pub flags: u8,
    pub name: &'a [u16],
}

/// Writes frames. Entries are gathered into `Entries` frames of up to [`MAX_FRAME`] bytes.
#[derive(Debug)]
pub struct Encoder<W: Write> {
    out: W,
    entries: Vec<u8>,
    pending: u32,
}

impl<W: Write> Encoder<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            entries: Vec::with_capacity(MAX_FRAME),
            pending: 0,
        }
    }

    fn frame(&mut self, tag: u8, payload: &[u8]) -> io::Result<()> {
        debug_assert!(payload.len() <= MAX_FRAME);
        self.out.write_all(&[tag])?;
        self.out.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.out.write_all(payload)
    }

    pub fn header(&mut self, h: &Header) -> io::Result<()> {
        let mut p = Vec::with_capacity(44);
        p.extend_from_slice(&MAGIC);
        p.extend_from_slice(&VERSION.to_le_bytes());
        p.extend_from_slice(&h.volume_serial.to_le_bytes());
        p.extend_from_slice(&h.cluster_size.to_le_bytes());
        p.extend_from_slice(&h.record_size.to_le_bytes());
        p.extend_from_slice(&h.records.to_le_bytes());
        p.extend_from_slice(&h.total_clusters.to_le_bytes());
        self.frame(TAG_HEADER, &p)
    }

    pub fn progress(&mut self, p: &Progress) -> io::Result<()> {
        let mut b = Vec::with_capacity(32);
        for v in [p.records_done, p.records_total, p.files, p.folders] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        self.frame(TAG_PROGRESS, &b)?;
        self.out.flush()
    }

    pub fn dir(&mut self, frn: u64) -> io::Result<()> {
        self.frame(TAG_DIR, &frn.to_le_bytes())
    }

    /// Adds an entry to the folder being listed. Names longer than [`MAX_NAME`] are the caller's to refuse.
    pub fn entry(&mut self, e: &OutEntry<'_>) -> io::Result<()> {
        let size = ENTRY_FIXED + e.name.len() * 2;
        if 4 + self.entries.len() + size > MAX_FRAME {
            self.flush_entries()?;
        }
        let b = &mut self.entries;
        b.extend_from_slice(&e.frn.to_le_bytes());
        b.extend_from_slice(&e.attributes.to_le_bytes());
        b.extend_from_slice(&e.tag.to_le_bytes());
        b.extend_from_slice(&e.logical.to_le_bytes());
        b.extend_from_slice(&e.allocated.to_le_bytes());
        b.extend_from_slice(&e.modified.to_le_bytes());
        b.extend_from_slice(&e.accessed.to_le_bytes());
        b.push(e.flags);
        b.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        for u in e.name {
            b.extend_from_slice(&u.to_le_bytes());
        }
        self.pending += 1;
        Ok(())
    }

    fn flush_entries(&mut self) -> io::Result<()> {
        if self.pending == 0 {
            return Ok(());
        }
        let mut p = Vec::with_capacity(4 + self.entries.len());
        p.extend_from_slice(&self.pending.to_le_bytes());
        p.extend_from_slice(&self.entries);
        self.entries.clear();
        self.pending = 0;
        self.frame(TAG_ENTRIES, &p)
    }

    pub fn dir_end(&mut self, frn: u64, count: u64) -> io::Result<()> {
        self.flush_entries()?;
        let mut p = Vec::with_capacity(16);
        p.extend_from_slice(&frn.to_le_bytes());
        p.extend_from_slice(&count.to_le_bytes());
        self.frame(TAG_DIR_END, &p)
    }

    pub fn locked(&mut self, l: &Locked) -> io::Result<()> {
        let mut p = Vec::with_capacity(32);
        for v in [l.frn, l.bytes, l.files, l.folders] {
            p.extend_from_slice(&v.to_le_bytes());
        }
        self.frame(TAG_LOCKED, &p)
    }

    pub fn records(&mut self, bytes: u64) -> io::Result<()> {
        self.frame(TAG_RECORDS, &bytes.to_le_bytes())
    }

    pub fn end(&mut self, e: &End) -> io::Result<()> {
        let mut p = Vec::with_capacity(24);
        for v in [e.dirs, e.entries, e.locked] {
            p.extend_from_slice(&v.to_le_bytes());
        }
        self.frame(TAG_END, &p)?;
        self.out.flush()
    }

    pub fn error(&mut self, code: ErrorCode) -> io::Result<()> {
        self.flush_entries()?;
        self.frame(TAG_ERROR, &code.code().to_le_bytes())?;
        self.out.flush()
    }

    /// The writer underneath.
    pub fn into_inner(self) -> W {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut enc = Encoder::new(Vec::new());
        let header = Header {
            volume_serial: 0xABCD,
            cluster_size: 4096,
            record_size: 1024,
            records: 100,
            total_clusters: 1 << 20,
        };
        enc.header(&header).unwrap();
        enc.dir(5).unwrap();
        let name: Vec<u16> = "café.txt".encode_utf16().collect();
        for i in 0..3 {
            enc.entry(&OutEntry {
                frn: 64 + i,
                attributes: 0x20,
                tag: 0,
                logical: 10,
                allocated: 16,
                modified: 1,
                accessed: 2,
                flags: 0,
                name: &name,
            })
            .unwrap();
        }
        enc.dir_end(5, 3).unwrap();
        enc.records(77).unwrap();
        enc.end(&End {
            dirs: 1,
            entries: 3,
            locked: 0,
        })
        .unwrap();
        let bytes = enc.into_inner();

        let mut dec = Decoder::new(&bytes[..]);
        assert!(matches!(dec.next_frame().unwrap(), Some(Frame::Header(h)) if h == header));
        assert!(matches!(
            dec.next_frame().unwrap(),
            Some(Frame::Dir { frn: 5 })
        ));
        match dec.next_frame().unwrap() {
            Some(Frame::Entries(entries)) => {
                let all: Vec<_> = entries.map(|e| e.unwrap()).collect();
                assert_eq!(all.len(), 3);
                assert_eq!(all[2].frn, 66);
                let mut n = Vec::new();
                all[0].name_into(&mut n);
                assert_eq!(String::from_utf16(&n).unwrap(), "café.txt");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            dec.next_frame().unwrap(),
            Some(Frame::DirEnd { frn: 5, count: 3 })
        ));
        assert!(matches!(
            dec.next_frame().unwrap(),
            Some(Frame::Records { bytes: 77 })
        ));
        assert!(matches!(dec.next_frame().unwrap(), Some(Frame::End(_))));
        assert!(dec.next_frame().unwrap().is_none());
    }

    #[test]
    fn big_folders_span_several_frames() {
        let mut enc = Encoder::new(Vec::new());
        let name = vec![u16::from(b'x'); 200];
        enc.dir(5).unwrap();
        for i in 0..10_000u64 {
            enc.entry(&OutEntry {
                frn: i,
                attributes: 0,
                tag: 0,
                logical: 0,
                allocated: 0,
                modified: 0,
                accessed: 0,
                flags: 0,
                name: &name,
            })
            .unwrap();
        }
        enc.dir_end(5, 10_000).unwrap();
        let bytes = enc.into_inner();
        let mut dec = Decoder::new(&bytes[..]);
        let mut frames = 0;
        let mut entries = 0;
        while let Some(f) = dec.next_frame().unwrap() {
            if let Frame::Entries(e) = f {
                frames += 1;
                entries += e.inspect(|e| assert!(e.is_ok())).count();
            }
        }
        assert!(frames > 1);
        assert_eq!(entries, 10_000);
    }

    #[test]
    fn refuses_malformed_frames() {
        let check = |bytes: &[u8]| {
            let mut dec = Decoder::new(bytes);
            loop {
                match dec.next_frame() {
                    Ok(Some(Frame::Entries(e))) => {
                        for entry in e {
                            if entry.is_err() {
                                return true;
                            }
                        }
                    }
                    Ok(Some(_)) => {}
                    Ok(None) => return false,
                    Err(_) => return true,
                }
            }
        };
        // Too long a frame.
        let mut b = vec![TAG_DIR];
        b.extend_from_slice(&(MAX_FRAME as u32 + 1).to_le_bytes());
        assert!(check(&b));
        // Cut off.
        assert!(check(&[TAG_DIR, 8, 0, 0, 0, 1, 2]));
        assert!(check(&[TAG_DIR, 8, 0]));
        // Unknown tag, wrong length for its fields.
        assert!(check(&[200, 0, 0, 0, 0]));
        assert!(check(&[TAG_DIR, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        // Entries claiming more than the frame holds, or a zero-length name.
        let mut b = vec![TAG_ENTRIES];
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&1000u32.to_le_bytes());
        assert!(check(&b));
        let mut payload = 1u32.to_le_bytes().to_vec();
        payload.extend_from_slice(&[0u8; ENTRY_FIXED]);
        payload.extend_from_slice(&[0, 0]);
        let mut b = vec![TAG_ENTRIES];
        b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        b.extend_from_slice(&payload);
        assert!(check(&b));
        // A header from another protocol.
        let mut b = vec![TAG_HEADER];
        b.extend_from_slice(&44u32.to_le_bytes());
        b.extend_from_slice(&[0u8; 44]);
        assert!(check(&b));
    }
}
