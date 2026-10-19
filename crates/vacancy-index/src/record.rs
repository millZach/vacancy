//! One MFT file record: its fixups undone, then the attributes fast scan needs read out of it. Nothing here reads
//! a file's contents: of `$DATA` only the sizes in its header are read, never its value.
//!
//! Layout (NTFS 3.1):
//! - the record header: `FILE`, the update sequence array, the sequence number, the link count, the offset of the
//!   first attribute, flags (in use, directory), the bytes used, and the base record for an extension record;
//! - attributes, each a header (type, length, resident or not, name, flags) and either a value in the record or
//!   data runs on the volume, until the type `0xFFFF_FFFF`.

use crate::le::{array, i64_at, slice, u8_at, u16_at, u32_at, u64_at, utf16_at};

/// Bytes between two update sequence numbers in a multi-sector record: NTFS uses 512 whatever the disk's sector.
pub const SECTOR: usize = 512;

pub const IN_USE: u16 = 0x0001;
pub const IS_DIRECTORY: u16 = 0x0002;

pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_SECURITY_DESCRIPTOR: u32 = 0x50;
pub const ATTR_DATA: u32 = 0x80;
pub const ATTR_REPARSE_POINT: u32 = 0xC0;
const ATTR_END: u32 = 0xFFFF_FFFF;

const FLAG_COMPRESSED: u16 = 0x0001;
const FLAG_SPARSE: u16 = 0x8000;

/// The `$FILE_NAME` namespace of DOS 8.3 names, which duplicate a long name and never appear in a listing.
pub const NAMESPACE_DOS: u8 = 2;

/// The name of the stream the Windows Overlay Filter keeps a compressed file's data in.
pub const WOF_STREAM: &str = "WofCompressedData";

/// Why a record couldn't be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordError {
    /// Not a `FILE` record: never written, or zeroed. Not an error for the volume.
    Empty,
    /// The update sequence numbers don't match: the record was being written as it was read, or is damaged.
    Torn,
    /// A field points outside the record, or an attribute is malformed.
    Malformed,
}

/// A record's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub sequence: u16,
    pub links: u16,
    pub flags: u16,
    /// For an extension record, the base record it holds attributes for (record number and sequence).
    pub base: Option<FileRef>,
}

impl Header {
    pub fn in_use(&self) -> bool {
        self.flags & IN_USE != 0
    }
    pub fn is_dir(&self) -> bool {
        self.flags & IS_DIRECTORY != 0
    }
}

/// A file reference: a record number (48 bits) and the sequence number it had (16 bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileRef {
    pub record: u64,
    pub sequence: u16,
}

impl FileRef {
    pub fn from_raw(raw: u64) -> Self {
        Self {
            record: raw & 0x0000_FFFF_FFFF_FFFF,
            sequence: (raw >> 48) as u16,
        }
    }

    /// As Windows gives it in a file id: the sequence in the top 16 bits.
    pub fn raw(self) -> u64 {
        (u64::from(self.sequence) << 48) | (self.record & 0x0000_FFFF_FFFF_FFFF)
    }
}

/// `$STANDARD_INFORMATION`: times, attributes and the security id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Standard {
    pub created: i64,
    pub modified: i64,
    pub changed: i64,
    pub accessed: i64,
    pub attributes: u32,
    /// Index into `$Secure`; `None` on volumes older than NTFS 3.0, whose records carry their own descriptor.
    pub security_id: Option<u32>,
}

/// A `$FILE_NAME`: one name of the file in one folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub parent: FileRef,
    pub namespace: u8,
    pub name: Vec<u16>,
}

/// A data stream's sizes, from its first extent's header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sizes {
    /// Length in bytes.
    pub logical: u64,
    /// Bytes allocated on the volume: for a compressed or sparse stream, the clusters it really has.
    pub allocated: u64,
}

/// Where one extent of a non-resident attribute is: its first VCN, how many clusters it covers, and the runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extent {
    pub lowest_vcn: u64,
    pub runs: Vec<crate::runs::Run>,
}

/// An attribute of interest, read out of a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attr {
    Standard(Standard),
    Name(Name),
    /// The unnamed `$DATA`: the file's own data. `sizes` only from the extent that starts at VCN 0 (or a
    /// resident value); later extents give `None`.
    Data {
        sizes: Option<Sizes>,
    },
    /// A named `$DATA` stream (alternate data stream, or the Windows Overlay Filter's): `sizes` as above, and
    /// what this extent takes on the volume.
    Stream {
        name: Vec<u16>,
        sizes: Option<Sizes>,
        on_disk: u64,
    },
    /// The reparse tag, when the reparse point's value is in the record.
    Reparse {
        tag: u32,
    },
    /// A `$SECURITY_DESCRIPTOR` held in the record (NTFS before 3.0): its bytes; `None` if non-resident.
    Security(Option<Vec<u8>>),
    /// Any other attribute's extent: what it takes on the volume (indexes, bitmaps, extended attributes, logged
    /// streams).
    Other {
        allocated: u64,
    },
    /// The extents of a non-resident attribute when asked for (the `$MFT`'s data, `$Secure:$SDS`).
    Runs {
        kind: u32,
        name: Vec<u16>,
        extent: Extent,
        sizes: Option<Sizes>,
    },
    /// An attribute list, resident (its entries) or not.
    List(Option<Vec<ListEntry>>),
}

/// One entry of an `$ATTRIBUTE_LIST`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEntry {
    pub kind: u32,
    pub name: Vec<u16>,
    pub lowest_vcn: u64,
    pub record: FileRef,
}

/// Applies the update sequence array to a record in place, as NTFS does before using it: the last two bytes of
/// each 512-byte stride hold the update sequence number, and the real bytes are kept in the array.
pub fn apply_fixups(record: &mut [u8]) -> Result<(), RecordError> {
    if record.get(0..4) != Some(b"FILE") {
        return Err(RecordError::Empty);
    }
    let usa_at = usize::from(u16_at(record, 4).ok_or(RecordError::Malformed)?);
    let usa_count = usize::from(u16_at(record, 6).ok_or(RecordError::Malformed)?);
    if !record.len().is_multiple_of(SECTOR)
        || usa_count != record.len() / SECTOR + 1
        || !usa_at.is_multiple_of(2)
    {
        return Err(RecordError::Malformed);
    }
    let end = usa_at + usa_count * 2;
    if end > SECTOR - 2 {
        return Err(RecordError::Malformed);
    }
    let usn = array::<2>(record, usa_at).ok_or(RecordError::Malformed)?;
    for i in 1..usa_count {
        let tail = i * SECTOR - 2;
        let original = array::<2>(record, usa_at + i * 2).ok_or(RecordError::Malformed)?;
        if record[tail..tail + 2] != usn {
            return Err(RecordError::Torn);
        }
        record[tail..tail + 2].copy_from_slice(&original);
    }
    Ok(())
}

/// Reads the header of a record whose fixups are applied.
pub fn header(record: &[u8]) -> Result<Header, RecordError> {
    let m = RecordError::Malformed;
    let base = u64_at(record, 0x20).ok_or(m)?;
    Ok(Header {
        sequence: u16_at(record, 0x10).ok_or(m)?,
        links: u16_at(record, 0x12).ok_or(m)?,
        flags: u16_at(record, 0x16).ok_or(m)?,
        base: (base & 0x0000_FFFF_FFFF_FFFF != 0).then(|| FileRef::from_raw(base)),
    })
}

/// What a caller wants of a record besides the usual attributes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Want {
    /// Report the extents of non-resident attributes as [`Attr::Runs`] (for `$MFT` and `$Secure`).
    pub runs: bool,
    /// The volume's size in clusters, to check the runs against.
    pub total_clusters: u64,
    /// The volume's cluster size, to turn clusters into bytes.
    pub cluster_size: u64,
}

/// Reads the attributes of a record whose fixups are applied, calling `each` for every one of interest.
pub fn attributes(
    record: &[u8],
    want: Want,
    each: &mut dyn FnMut(Attr),
) -> Result<(), RecordError> {
    let m = RecordError::Malformed;
    let used = u32_at(record, 0x18).ok_or(m)? as usize;
    if used > record.len() || used < 0x30 {
        return Err(m);
    }
    let record = &record[..used];
    let mut at = usize::from(u16_at(record, 0x14).ok_or(m)?);
    if at < 0x30 || !at.is_multiple_of(8) {
        return Err(m);
    }
    loop {
        let kind = u32_at(record, at).ok_or(m)?;
        if kind == ATTR_END {
            return Ok(());
        }
        let len = u32_at(record, at + 4).ok_or(m)? as usize;
        if len < 0x18 || !len.is_multiple_of(8) {
            return Err(m);
        }
        let attr = slice(record, at, len).ok_or(m)?;
        read_attribute(kind, attr, want, each)?;
        at += len;
    }
}

fn read_attribute(
    kind: u32,
    a: &[u8],
    want: Want,
    each: &mut dyn FnMut(Attr),
) -> Result<(), RecordError> {
    let m = RecordError::Malformed;
    let non_resident = u8_at(a, 8).ok_or(m)? != 0;
    let name_len = usize::from(u8_at(a, 9).ok_or(m)?);
    let name_at = usize::from(u16_at(a, 0x0A).ok_or(m)?);
    let flags = u16_at(a, 0x0C).ok_or(m)?;
    let name = if name_len == 0 {
        Vec::new()
    } else {
        utf16_at(a, name_at, name_len).ok_or(m)?
    };

    if non_resident {
        let lowest = u64_at(a, 0x10).ok_or(m)?;
        let highest = u64_at(a, 0x18).ok_or(m)?;
        let runs_at = usize::from(u16_at(a, 0x20).ok_or(m)?);
        let allocated = u64_at(a, 0x28).ok_or(m)?;
        let logical = u64_at(a, 0x30).ok_or(m)?;
        // The real allocation of a compressed or sparse stream follows the header when it has one.
        let total = if flags & (FLAG_COMPRESSED | FLAG_SPARSE) != 0 && runs_at >= 0x48 {
            u64_at(a, 0x40).ok_or(m)?
        } else {
            allocated
        };
        if runs_at > a.len() || logical > allocated.max(total).saturating_add(1 << 40) {
            return Err(m);
        }
        // Only the first extent's header gives the stream's sizes.
        let sizes = (lowest == 0).then_some(Sizes {
            logical,
            allocated: total,
        });
        // An empty attribute has a highest VCN of -1.
        let clusters = (highest as i64)
            .checked_sub(lowest as i64)
            .and_then(|d| d.checked_add(1))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or(m)?;
        // What this extent really takes on the volume, from its runs (holes take nothing): for everything that
        // isn't a file's own data, whose header may claim a whole volume (`$BadClus:$Bad` is one big hole).
        let unnamed_data = kind == ATTR_DATA && name.is_empty();
        let runs = if want.total_clusters > 0 && !unnamed_data {
            Some(crate::runs::decode(&a[runs_at..], want.total_clusters, clusters).ok_or(m)?)
        } else {
            None
        };
        let on_disk = runs.as_ref().map_or(0, |r| {
            crate::runs::allocated_clusters(r).saturating_mul(want.cluster_size)
        });
        // A security descriptor too big for its record (volumes written by other systems) is read from its runs.
        let far_sd = kind == ATTR_SECURITY_DESCRIPTOR && lowest == 0 && want.total_clusters > 0;
        if far_sd || (want.runs && matches!(kind, ATTR_DATA | ATTR_ATTRIBUTE_LIST)) {
            let runs = match runs {
                Some(r) => r,
                None => {
                    crate::runs::decode(&a[runs_at..], want.total_clusters, clusters).ok_or(m)?
                }
            };
            each(Attr::Runs {
                kind,
                name: name.clone(),
                extent: Extent {
                    lowest_vcn: lowest,
                    runs,
                },
                sizes,
            });
        }
        match kind {
            ATTR_DATA if name.is_empty() => each(Attr::Data { sizes }),
            ATTR_DATA => each(Attr::Stream {
                name,
                sizes,
                on_disk,
            }),
            ATTR_SECURITY_DESCRIPTOR => {
                each(Attr::Security(None));
                each(Attr::Other { allocated: on_disk });
            }
            ATTR_ATTRIBUTE_LIST => {
                each(Attr::List(None));
                each(Attr::Other { allocated: on_disk });
            }
            // A reparse point too big for the record (its tag can't be read here), indexes, bitmaps, extended
            // attributes, logged streams.
            _ => each(Attr::Other { allocated: on_disk }),
        }
        return Ok(());
    }

    let value_len = u32_at(a, 0x10).ok_or(m)? as usize;
    let value_at = usize::from(u16_at(a, 0x14).ok_or(m)?);
    let value = slice(a, value_at, value_len).ok_or(m)?;
    match kind {
        ATTR_STANDARD_INFORMATION => {
            if value.len() < 0x24 {
                return Err(m);
            }
            each(Attr::Standard(Standard {
                created: i64_at(value, 0).ok_or(m)?,
                modified: i64_at(value, 0x08).ok_or(m)?,
                changed: i64_at(value, 0x10).ok_or(m)?,
                accessed: i64_at(value, 0x18).ok_or(m)?,
                attributes: u32_at(value, 0x20).ok_or(m)?,
                security_id: (value.len() >= 0x48).then(|| u32_at(value, 0x34)).flatten(),
            }));
        }
        ATTR_FILE_NAME => {
            let parent = FileRef::from_raw(u64_at(value, 0).ok_or(m)?);
            let units = usize::from(u8_at(value, 0x40).ok_or(m)?);
            let namespace = u8_at(value, 0x41).ok_or(m)?;
            let name = utf16_at(value, 0x42, units).ok_or(m)?;
            if namespace > 3 {
                return Err(m);
            }
            each(Attr::Name(Name {
                parent,
                namespace,
                name,
            }));
        }
        ATTR_DATA => {
            // A resident stream: only its length is read, never its bytes.
            let sizes = Some(Sizes {
                logical: value_len as u64,
                allocated: resident_allocation(value_len as u64),
            });
            if name.is_empty() {
                each(Attr::Data { sizes });
            } else {
                each(Attr::Stream {
                    name,
                    sizes,
                    on_disk: 0,
                });
            }
        }
        ATTR_REPARSE_POINT => each(Attr::Reparse {
            tag: u32_at(value, 0).ok_or(m)?,
        }),
        ATTR_SECURITY_DESCRIPTOR => each(Attr::Security(Some(value.to_vec()))),
        ATTR_ATTRIBUTE_LIST => each(Attr::List(Some(list_entries(value).ok_or(m)?))),
        _ => {}
    }
    Ok(())
}

/// What Windows' listings report as allocated for a stream kept inside its record: its length rounded up to eight
/// bytes (measured on Windows 11: a 5-byte file lists 8 allocated, a 700-byte one 704).
pub fn resident_allocation(len: u64) -> u64 {
    len.saturating_add(7) & !7
}

/// The entries of an attribute list's value.
pub fn list_entries(v: &[u8]) -> Option<Vec<ListEntry>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < v.len() {
        let kind = u32_at(v, at)?;
        let len = usize::from(u16_at(v, at + 4)?);
        let name_len = usize::from(u8_at(v, at + 6)?);
        let name_at = usize::from(u8_at(v, at + 7)?);
        if len < 0x1A || at + len > v.len() {
            return None;
        }
        let entry = &v[at..at + len];
        out.push(ListEntry {
            kind,
            name: if name_len == 0 {
                Vec::new()
            } else {
                utf16_at(entry, name_at, name_len)?
            },
            lowest_vcn: u64_at(entry, 8)?,
            record: FileRef::from_raw(u64_at(entry, 0x10)?),
        });
        at += len;
    }
    Some(out)
}
