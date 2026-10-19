//! The index in memory: one compact row per MFT record and one per name, gathered as the records are read, in any
//! order. An extension record's attributes go to its base record's row.
//!
//! About 40 bytes a record and 20 a name, plus the names themselves (one byte a character when they are all Latin-1):
//! some 400 MB for five million files, held for the seconds the read takes.

use std::collections::HashMap;

use crate::record::{
    self, ATTR_DATA, Attr, Extent, FileRef, IS_DIRECTORY, NAMESPACE_DOS, RecordError, Sizes,
    WOF_STREAM, Want,
};

/// No record, no name.
pub(crate) const NONE: u32 = u32::MAX;

/// Row flags.
pub(crate) const F_IN_USE: u16 = 1;
pub(crate) const F_DIR: u16 = 1 << 1;
/// Its base record was read (`$STANDARD_INFORMATION` seen): without it the record is never listed.
pub(crate) const F_STANDARD: u16 = 1 << 2;
pub(crate) const F_REPARSE: u16 = 1 << 3;
pub(crate) const F_WOF_STREAM: u16 = 1 << 4;
pub(crate) const F_INLINE_SD: u16 = 1 << 5;
/// A non-resident `$SECURITY_DESCRIPTOR`: its descriptor can't be read here.
pub(crate) const F_FAR_SD: u16 = 1 << 6;
/// Set while emitting: a folder that already has its one name in the tree.
pub(crate) const F_NAMED: u16 = 1 << 7;
/// Set while emitting: one of its names is in a visible folder.
pub(crate) const F_VISIBLE: u16 = 1 << 8;
/// Set while emitting: already counted in a total.
pub(crate) const F_COUNTED: u16 = 1 << 9;
/// Set while reading: the base record itself was read.
pub(crate) const F_BASE_READ: u16 = 1 << 10;

/// One record.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Row {
    pub(crate) sequence: u16,
    pub(crate) flags: u16,
    pub(crate) attributes: u32,
    pub(crate) security_id: u32,
    /// Its latest name, or [`NONE`]: whether it has one.
    pub(crate) first_name: u32,
    /// Unix seconds.
    pub(crate) modified: u32,
    pub(crate) accessed: u32,
    pub(crate) logical: u64,
    pub(crate) allocated: u64,
}

const EMPTY_ROW: Row = Row {
    sequence: 0,
    flags: 0,
    attributes: 0,
    security_id: 0,
    first_name: NONE,
    modified: 0,
    accessed: 0,
    logical: 0,
    allocated: 0,
};

/// One name.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NameRow {
    pub(crate) owner: u32,
    pub(crate) parent: u32,
    pub(crate) next_in_parent: u32,
    start: u32,
    units: u8,
    /// Bit 0: stored one byte a unit.
    narrow: u8,
    pub(crate) parent_sequence: u16,
}

/// What was gathered from the records.
#[derive(Debug)]
pub struct Table {
    pub(crate) rows: Vec<Row>,
    pub(crate) names: Vec<NameRow>,
    arena: Vec<u8>,
    /// Reparse tags, by record.
    pub(crate) tags: HashMap<u32, u32>,
    /// The Windows Overlay Filter's compressed data's allocation, by record.
    pub(crate) wof: HashMap<u32, u64>,
    /// Security descriptors kept in records (volumes older than NTFS 3.0, or written by other systems), by record.
    pub(crate) inline_sd: HashMap<u32, Vec<u8>>,
    /// Security descriptors too big for their record: their runs and length, read by [`Table::far_descriptors`]'s
    /// caller.
    pub(crate) far_sd: HashMap<u32, (Vec<crate::runs::Run>, u64)>,
    /// Bytes on the volume of attributes that aren't a file's own data: folder indexes, alternate streams,
    /// attribute lists, extended attributes. Part of "File system records".
    pub(crate) extra_bytes: u64,
    /// `$Secure:$SDS`'s extents and size, gathered from record 9 and its extension records.
    pub(crate) sds: Vec<Extent>,
    pub(crate) sds_size: Option<u64>,
    /// Extension records' attributes, with the base record and sequence they name, applied by [`Table::finish`].
    extensions: Vec<(u32, u16, Vec<Attr>)>,
    /// More names than 32-bit offsets reach: the index is refused.
    pub overflow: bool,
    /// Records that couldn't be read.
    pub torn: u64,
    pub malformed: u64,
    /// In-use base records with a name, files and folders, for progress.
    pub files: u64,
    pub folders: u64,
    total_clusters: u64,
    cluster_size: u64,
}

/// The record that holds `$Secure`.
pub(crate) const SECURE: u32 = 9;

impl Table {
    /// A table for `records` records, or `None` if there isn't the memory for it.
    pub fn new(records: u32, total_clusters: u64, cluster_size: u64) -> Option<Self> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(records as usize).ok()?;
        rows.resize(records as usize, EMPTY_ROW);
        Some(Self {
            rows,
            names: Vec::new(),
            arena: Vec::new(),
            tags: HashMap::new(),
            wof: HashMap::new(),
            inline_sd: HashMap::new(),
            far_sd: HashMap::new(),
            extra_bytes: 0,
            sds: Vec::new(),
            sds_size: None,
            extensions: Vec::new(),
            overflow: false,
            torn: 0,
            malformed: 0,
            files: 0,
            folders: 0,
            total_clusters,
            cluster_size,
        })
    }

    /// Records in the table.
    pub fn len(&self) -> u32 {
        self.rows.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Reads record `number` from `buf` (its raw bytes; fixups are applied here) into the table.
    pub fn add(&mut self, number: u32, buf: &mut [u8]) {
        match self.try_add(number, buf) {
            Ok(()) | Err(RecordError::Empty) => {}
            Err(RecordError::Torn) => self.torn += 1,
            Err(RecordError::Malformed) => self.malformed += 1,
        }
    }

    fn try_add(&mut self, number: u32, buf: &mut [u8]) -> Result<(), RecordError> {
        if number as usize >= self.rows.len() {
            return Err(RecordError::Malformed);
        }
        record::apply_fixups(buf)?;
        let header = record::header(buf)?;
        if !header.in_use() {
            return Ok(());
        }
        let owner = match header.base {
            None => number,
            Some(base) => {
                let b = u32::try_from(base.record).map_err(|_| RecordError::Malformed)?;
                if b as usize >= self.rows.len() || b == number {
                    return Err(RecordError::Malformed);
                }
                b
            }
        };
        let want = Want {
            runs: owner == SECURE,
            total_clusters: self.total_clusters,
            cluster_size: self.cluster_size,
        };
        // Gather first, so a malformed record adds nothing.
        let mut attrs = Vec::new();
        record::attributes(buf, want, &mut |a| attrs.push(a))?;
        if let Some(base) = header.base {
            // Applied once every record is read, and only if the base record is in use with the sequence this
            // extension names: a stale extension never lends a name or data to a reused record.
            self.extensions.push((owner, base.sequence, attrs));
            return Ok(());
        }
        {
            let row = &mut self.rows[owner as usize];
            row.sequence = header.sequence;
            row.flags |= F_IN_USE | F_BASE_READ;
            if header.flags & IS_DIRECTORY != 0 {
                row.flags |= F_DIR;
            }
        }
        for a in attrs {
            self.apply(owner, a);
        }
        let row = self.rows[owner as usize];
        if row.first_name != NONE {
            if row.flags & F_DIR != 0 {
                self.folders += 1;
            } else {
                self.files += 1;
            }
        }
        Ok(())
    }

    /// Applies the extension records' attributes to their base records: call once every record is read.
    pub fn finish(&mut self) {
        for (owner, sequence, attrs) in std::mem::take(&mut self.extensions) {
            let row = self.rows[owner as usize];
            if row.flags & F_BASE_READ == 0 || row.sequence != sequence {
                self.malformed += 1;
                continue;
            }
            for a in attrs {
                self.apply(owner, a);
            }
        }
    }

    fn apply(&mut self, owner: u32, a: Attr) {
        let i = owner as usize;
        match a {
            Attr::Standard(s) => {
                let row = &mut self.rows[i];
                row.flags |= F_STANDARD;
                row.attributes = s.attributes;
                row.security_id = s.security_id.unwrap_or(0);
                row.modified = unix_seconds(s.modified);
                row.accessed = unix_seconds(s.accessed);
            }
            Attr::Name(n) => {
                if n.namespace == NAMESPACE_DOS || n.name.is_empty() || n.name.len() > 255 {
                    return;
                }
                let Ok(parent) = u32::try_from(n.parent.record) else {
                    return;
                };
                // Offsets and ids are 32-bit: an index too big for them is refused whole, never wrapped.
                let (Ok(start), Ok(id)) = (
                    u32::try_from(self.arena.len()),
                    u32::try_from(self.names.len()),
                ) else {
                    self.overflow = true;
                    return;
                };
                if id == NONE || start.checked_add(2 * 255).is_none() {
                    self.overflow = true;
                    return;
                }
                let narrow = n.name.iter().all(|&u| u < 0x100);
                if narrow {
                    self.arena.extend(n.name.iter().map(|&u| u as u8));
                } else {
                    for u in &n.name {
                        self.arena.extend_from_slice(&u.to_le_bytes());
                    }
                }
                self.names.push(NameRow {
                    owner,
                    parent,
                    next_in_parent: NONE,
                    start,
                    units: n.name.len() as u8,
                    narrow: u8::from(narrow),
                    parent_sequence: n.parent.sequence,
                });
                self.rows[i].first_name = id;
            }
            Attr::Data { sizes: Some(s) } => {
                let row = &mut self.rows[i];
                row.logical = s.logical;
                row.allocated = s.allocated;
            }
            Attr::Data { sizes: None } => {}
            Attr::Stream {
                name,
                sizes,
                on_disk,
            } => {
                if name.iter().copied().eq(WOF_STREAM.encode_utf16()) {
                    self.rows[i].flags |= F_WOF_STREAM;
                    if let Some(s) = sizes {
                        self.wof.insert(owner, s.allocated);
                    }
                } else {
                    self.extra_bytes = self.extra_bytes.saturating_add(on_disk);
                }
            }
            Attr::Reparse { tag } => {
                self.rows[i].flags |= F_REPARSE;
                self.tags.insert(owner, tag);
            }
            Attr::Security(Some(sd)) => {
                self.rows[i].flags |= F_INLINE_SD;
                self.inline_sd.insert(owner, sd);
            }
            Attr::Security(None) => self.rows[i].flags |= F_FAR_SD,
            Attr::Other { allocated } => {
                self.extra_bytes = self.extra_bytes.saturating_add(allocated);
            }
            Attr::Runs {
                kind,
                name,
                extent,
                sizes,
            } => {
                if kind == record::ATTR_SECURITY_DESCRIPTOR {
                    if let Some(Sizes { logical, .. }) = sizes {
                        self.far_sd.insert(owner, (extent.runs, logical));
                    }
                    return;
                }
                if owner == SECURE
                    && kind == ATTR_DATA
                    && name.iter().copied().eq("$SDS".encode_utf16())
                {
                    if let Some(Sizes { logical, .. }) = sizes {
                        self.sds_size = Some(logical);
                    }
                    self.sds.push(extent);
                }
            }
            Attr::List(_) => {}
        }
    }

    /// The security descriptors too big for their records, to be read: by record, their runs and length.
    pub fn far_descriptors(&self) -> impl Iterator<Item = (u32, &[crate::runs::Run], u64)> + '_ {
        self.far_sd
            .iter()
            .map(|(&r, (runs, len))| (r, runs.as_slice(), *len))
    }

    /// Gives a far descriptor's bytes, read: the record's folder is then checked against them.
    pub fn set_descriptor(&mut self, record: u32, sd: Vec<u8>) {
        if let Some(row) = self.rows.get_mut(record as usize) {
            row.flags = (row.flags & !F_FAR_SD) | F_INLINE_SD;
            self.inline_sd.insert(record, sd);
        }
    }

    /// A name's UTF-16 units, into `out`.
    pub(crate) fn name_into(&self, n: &NameRow, out: &mut Vec<u16>) {
        out.clear();
        let start = n.start as usize;
        let units = usize::from(n.units);
        if n.narrow & 1 != 0 {
            out.extend(
                self.arena[start..start + units]
                    .iter()
                    .map(|&b| u16::from(b)),
            );
        } else {
            out.extend(
                self.arena[start..start + units * 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&c| u16::from_le_bytes(c)),
            );
        }
    }

    /// The file reference of record `i`.
    pub(crate) fn frn(&self, i: u32) -> u64 {
        FileRef {
            record: u64::from(i),
            sequence: self.rows[i as usize].sequence,
        }
        .raw()
    }
}

/// FILETIME to Unix seconds, clamped to what a `u32` holds (zero before 1970).
pub(crate) fn unix_seconds(filetime: i64) -> u32 {
    const TICKS: i64 = 10_000_000;
    const EPOCH_GAP: i64 = 11_644_473_600;
    if filetime <= 0 {
        return 0;
    }
    u32::try_from((filetime / TICKS - EPOCH_GAP).max(0)).unwrap_or(u32::MAX)
}

/// Unix seconds back to a FILETIME, as listings give times.
pub(crate) fn filetime(seconds: u32) -> i64 {
    const EPOCH_GAP: i64 = 11_644_473_600;
    if seconds == 0 {
        0
    } else {
        (i64::from(seconds) + EPOCH_GAP) * 10_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_round_trip_to_the_second() {
        // 2024-01-01T00:00:00Z plus a fraction.
        let ft = 133_485_408_000_000_000 + 1234;
        assert_eq!(unix_seconds(ft), 1_704_067_200);
        assert_eq!(unix_seconds(filetime(1_704_067_200)), 1_704_067_200);
        assert_eq!(unix_seconds(0), 0);
        assert_eq!(unix_seconds(-5), 0);
        assert_eq!(filetime(0), 0);
    }
}
