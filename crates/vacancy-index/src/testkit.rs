//! Synthetic NTFS volumes for tests: a boot sector, an MFT of hand-made records, and `$Secure:$SDS`. Enough of NTFS
//! for the reader to treat it as a volume, with every field the reader reads set as Windows sets it.

use crate::record::FileRef;

pub const CLUSTER: u64 = 4096;
pub const RECORD: usize = 1024;
/// Where the MFT starts, in clusters.
const MFT_LCN: u64 = 4;

/// One attribute's bytes.
#[derive(Clone, Debug)]
pub struct AttrBytes(pub Vec<u8>);

fn attr_header(kind: u32, non_resident: bool, name: &[u16], flags: u16, body_at: usize) -> Vec<u8> {
    let mut a = vec![0u8; body_at];
    a[0..4].copy_from_slice(&kind.to_le_bytes());
    a[8] = u8::from(non_resident);
    a[9] = name.len() as u8;
    a[0x0C..0x0E].copy_from_slice(&flags.to_le_bytes());
    a
}

fn finish(mut a: Vec<u8>) -> AttrBytes {
    while !a.len().is_multiple_of(8) {
        a.push(0);
    }
    let len = a.len() as u32;
    a[4..8].copy_from_slice(&len.to_le_bytes());
    AttrBytes(a)
}

fn resident(kind: u32, name: &[u16], value: &[u8]) -> AttrBytes {
    let name_at = 0x18;
    let value_at = (name_at + name.len() * 2 + 7) & !7;
    let mut a = attr_header(kind, false, name, 0, value_at);
    a[0x0A..0x0C].copy_from_slice(&(name_at as u16).to_le_bytes());
    for (i, u) in name.iter().enumerate() {
        a[name_at + i * 2..name_at + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
    }
    a[0x10..0x14].copy_from_slice(&(value.len() as u32).to_le_bytes());
    a[0x14..0x16].copy_from_slice(&(value_at as u16).to_le_bytes());
    a.extend_from_slice(value);
    finish(a)
}

/// `$STANDARD_INFORMATION` with these attributes, security id and times (FILETIMEs).
pub fn standard(attributes: u32, security_id: u32, modified: i64, accessed: i64) -> AttrBytes {
    let mut v = vec![0u8; 0x48];
    v[0x00..0x08].copy_from_slice(&modified.to_le_bytes());
    v[0x08..0x10].copy_from_slice(&modified.to_le_bytes());
    v[0x10..0x18].copy_from_slice(&modified.to_le_bytes());
    v[0x18..0x20].copy_from_slice(&accessed.to_le_bytes());
    v[0x20..0x24].copy_from_slice(&attributes.to_le_bytes());
    v[0x34..0x38].copy_from_slice(&security_id.to_le_bytes());
    resident(0x10, &[], &v)
}

/// `$FILE_NAME` in `parent`, in `namespace` (0 POSIX, 1 Win32, 2 DOS, 3 both).
pub fn file_name(parent: FileRef, namespace: u8, name: &str) -> AttrBytes {
    let units: Vec<u16> = name.encode_utf16().collect();
    let mut v = vec![0u8; 0x42 + units.len() * 2];
    v[0..8].copy_from_slice(&parent.raw().to_le_bytes());
    v[0x40] = units.len() as u8;
    v[0x41] = namespace;
    for (i, u) in units.iter().enumerate() {
        v[0x42 + i * 2..0x44 + i * 2].copy_from_slice(&u.to_le_bytes());
    }
    resident(0x30, &[], &v)
}

/// A resident `$DATA` (unnamed when `name` is empty) of `len` bytes, all `fill`.
pub fn data_resident(name: &str, len: usize, fill: u8) -> AttrBytes {
    let name: Vec<u16> = name.encode_utf16().collect();
    resident(0x80, &name, &vec![fill; len])
}

/// A non-resident attribute: the first extent (VCN 0) of `size` bytes, `allocated` allocated (and `total` really
/// allocated when compressed or sparse), at runs `runs` (raw mapping pairs, terminated).
#[allow(clippy::too_many_arguments)]
pub fn non_resident(
    kind: u32,
    name: &str,
    flags: u16,
    lowest_vcn: u64,
    highest_vcn: i64,
    size: u64,
    allocated: u64,
    total: Option<u64>,
    runs: &[u8],
) -> AttrBytes {
    let name: Vec<u16> = name.encode_utf16().collect();
    let header = if total.is_some() { 0x48 } else { 0x40 };
    let name_at = header;
    let runs_at = (name_at + name.len() * 2 + 7) & !7;
    let mut a = attr_header(kind, true, &name, flags, runs_at);
    a[0x0A..0x0C].copy_from_slice(&(name_at as u16).to_le_bytes());
    for (i, u) in name.iter().enumerate() {
        a[name_at + i * 2..name_at + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
    }
    a[0x10..0x18].copy_from_slice(&lowest_vcn.to_le_bytes());
    a[0x18..0x20].copy_from_slice(&highest_vcn.to_le_bytes());
    a[0x20..0x22].copy_from_slice(&(runs_at as u16).to_le_bytes());
    a[0x28..0x30].copy_from_slice(&allocated.to_le_bytes());
    a[0x30..0x38].copy_from_slice(&size.to_le_bytes());
    a[0x38..0x40].copy_from_slice(&size.to_le_bytes());
    if let Some(t) = total {
        a[0x40..0x48].copy_from_slice(&t.to_le_bytes());
    }
    a.extend_from_slice(runs);
    finish(a)
}

/// Mapping pairs for one run of `clusters` at `lcn` (or a hole), terminated.
pub fn one_run(lcn: Option<u64>, clusters: u64) -> Vec<u8> {
    let len = clusters.to_le_bytes();
    let len_n = 8 - (clusters.leading_zeros() / 8) as usize;
    let mut out = Vec::new();
    match lcn {
        Some(lcn) => {
            // Offsets are signed: one byte more when the top bit would be set.
            let mut off_n = 8 - (lcn.leading_zeros() / 8) as usize;
            if off_n < 8 && lcn.to_le_bytes()[off_n.max(1) - 1] & 0x80 != 0 {
                off_n += 1;
            }
            let off_n = off_n.max(1);
            out.push(((off_n as u8) << 4) | len_n as u8);
            out.extend_from_slice(&len[..len_n]);
            out.extend_from_slice(&lcn.to_le_bytes()[..off_n]);
        }
        None => {
            out.push(len_n as u8);
            out.extend_from_slice(&len[..len_n]);
        }
    }
    out.push(0);
    out
}

/// A `$REPARSE_POINT` with `tag`.
pub fn reparse(tag: u32) -> AttrBytes {
    let mut v = vec![0u8; 8];
    v[0..4].copy_from_slice(&tag.to_le_bytes());
    resident(0xC0, &[], &v)
}

/// An index root, as a folder has (its contents are never read).
pub fn index_root() -> AttrBytes {
    let name: Vec<u16> = "$I30".encode_utf16().collect();
    resident(0x90, &name, &[0u8; 32])
}

/// A record: header flags (`1` in use, `2` folder), sequence, base record, attributes. Returns its bytes with the
/// update sequence array applied as it is on disk.
pub fn record(
    number: u32,
    sequence: u16,
    flags: u16,
    base: Option<FileRef>,
    attrs: &[AttrBytes],
) -> Vec<u8> {
    let mut r = vec![0u8; RECORD];
    r[0..4].copy_from_slice(b"FILE");
    let usa_at = 0x30u16;
    let usa_count = (RECORD / 512 + 1) as u16;
    r[4..6].copy_from_slice(&usa_at.to_le_bytes());
    r[6..8].copy_from_slice(&usa_count.to_le_bytes());
    r[0x10..0x12].copy_from_slice(&sequence.to_le_bytes());
    r[0x12..0x14].copy_from_slice(&1u16.to_le_bytes());
    let first = 0x38u16;
    r[0x14..0x16].copy_from_slice(&first.to_le_bytes());
    r[0x16..0x18].copy_from_slice(&flags.to_le_bytes());
    r[0x1C..0x20].copy_from_slice(&(RECORD as u32).to_le_bytes());
    r[0x20..0x28].copy_from_slice(&base.map_or(0, FileRef::raw).to_le_bytes());
    r[0x2C..0x30].copy_from_slice(&number.to_le_bytes());
    let mut at = first as usize;
    for a in attrs {
        assert!(
            at + a.0.len() + 8 <= RECORD,
            "attributes overflow record {number}"
        );
        r[at..at + a.0.len()].copy_from_slice(&a.0);
        at += a.0.len();
    }
    r[at..at + 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    at += 8;
    r[0x18..0x1C].copy_from_slice(&(at as u32).to_le_bytes());
    // The update sequence: each sector's last two bytes move into the array.
    let usn = [0x34u8, 0x12];
    r[usa_at as usize..usa_at as usize + 2].copy_from_slice(&usn);
    for i in 1..usa_count as usize {
        let tail = i * 512 - 2;
        let orig = [r[tail], r[tail + 1]];
        r[usa_at as usize + i * 2..usa_at as usize + i * 2 + 2].copy_from_slice(&orig);
        r[tail..tail + 2].copy_from_slice(&usn);
    }
    r
}

/// A security descriptor's stand-in for tests: the [`crate::Access`] in tests reads its first byte.
pub fn fake_sd(allow: bool) -> Vec<u8> {
    let mut sd = vec![0u8; 40];
    sd[0] = u8::from(allow);
    sd
}

/// Builds a volume image: boot sector, MFT of `records` slots, `$SDS` holding `sds` (id, descriptor) pairs.
#[derive(Debug)]
pub struct Volume {
    pub records: Vec<Option<Vec<u8>>>,
    pub sds: Vec<(u32, Vec<u8>)>,
    /// Extra clusters of data the files' runs may point at (never read).
    pub data_clusters: u64,
}

impl Volume {
    pub fn new(slots: usize) -> Self {
        Self {
            records: vec![None; slots],
            sds: Vec::new(),
            data_clusters: 128,
        }
    }

    /// The MFT's size in clusters.
    fn mft_clusters(&self) -> u64 {
        (self.records.len() * RECORD) as u64 / CLUSTER
    }

    /// Where `$SDS` goes, in clusters.
    pub fn sds_lcn(&self) -> u64 {
        MFT_LCN + self.mft_clusters()
    }

    fn sds_bytes(&self) -> Vec<u8> {
        let mut s = Vec::new();
        for (id, sd) in &self.sds {
            let at = s.len();
            s.extend(crate::sds::entry(at, *id, sd));
        }
        s
    }

    /// Sets the system records every volume has: `$MFT` (0), the root (5), `$Secure` (9) and `$Extend` (11), and
    /// marks 1 to 15 in use. `root_sd` is the root's security id.
    pub fn system(&mut self, root_sd: u32) {
        let mft_bytes = self.mft_clusters() * CLUSTER;
        let me = |n: u32| FileRef {
            record: u64::from(n),
            sequence: n as u16 + 1,
        };
        let root = me(5);
        self.records[0] = Some(record(
            0,
            1,
            1,
            None,
            &[
                standard(0x6, 0x100, 1, 1),
                file_name(root, 3, "$MFT"),
                non_resident(
                    0x80,
                    "",
                    0,
                    0,
                    self.mft_clusters() as i64 - 1,
                    mft_bytes,
                    mft_bytes,
                    None,
                    &one_run(Some(MFT_LCN), self.mft_clusters()),
                ),
            ],
        ));
        for n in [1u32, 2, 3, 4, 6, 7, 8, 10, 12, 13, 14, 15] {
            self.records[n as usize] = Some(record(
                n,
                me(n).sequence,
                1,
                None,
                &[
                    standard(0x6, 0x100, 1, 1),
                    file_name(root, 3, &format!("$sys{n}")),
                ],
            ));
        }
        self.records[5] = Some(record(
            5,
            root.sequence,
            3,
            None,
            &[
                standard(0x6, root_sd, 1, 1),
                file_name(root, 3, "."),
                index_root(),
            ],
        ));
        let sds_len = self.sds_bytes().len().max(1) as u64;
        let sds_clusters = sds_len.div_ceil(CLUSTER);
        self.records[9] = Some(record(
            9,
            me(9).sequence,
            1,
            None,
            &[
                standard(0x6, 0x100, 1, 1),
                file_name(root, 3, "$Secure"),
                non_resident(
                    0x80,
                    "$SDS",
                    0,
                    0,
                    sds_clusters as i64 - 1,
                    sds_len,
                    sds_clusters * CLUSTER,
                    None,
                    &one_run(Some(self.sds_lcn()), sds_clusters),
                ),
            ],
        ));
        self.records[11] = Some(record(
            11,
            me(11).sequence,
            3,
            None,
            &[
                standard(0x6, 0x100, 1, 1),
                file_name(root, 3, "$Extend"),
                index_root(),
            ],
        ));
    }

    /// The root's file reference.
    pub fn root() -> FileRef {
        FileRef {
            record: 5,
            sequence: 6,
        }
    }

    /// The image's bytes.
    pub fn image(&self) -> Vec<u8> {
        let sds = self.sds_bytes();
        let sds_clusters = (sds.len().max(1) as u64).div_ceil(CLUSTER);
        let total = self.sds_lcn() + sds_clusters + self.data_clusters;
        let mut img = vec![0u8; (total * CLUSTER) as usize];
        let boot = crate::boot::sample(CLUSTER as u32, total, MFT_LCN);
        img[..512].copy_from_slice(&boot);
        let base = (MFT_LCN * CLUSTER) as usize;
        for (i, r) in self.records.iter().enumerate() {
            if let Some(r) = r {
                img[base + i * RECORD..base + (i + 1) * RECORD].copy_from_slice(r);
            }
        }
        let at = (self.sds_lcn() * CLUSTER) as usize;
        img[at..at + sds.len()].copy_from_slice(&sds);
        img
    }
}

/// An [`crate::Access`] for tests: a descriptor whose first byte is 1 can be listed.
#[derive(Debug, Default)]
pub struct FirstByte {
    pub asked: usize,
}

impl crate::Access for FirstByte {
    fn can_list(&mut self, security: crate::Security<'_>) -> bool {
        self.asked += 1;
        matches!(security, crate::Security::Descriptor(sd) if sd.first() == Some(&1))
    }
}

const ALLOW: u32 = 0x100;
const DENY: u32 = 0x101;
const FILE: u32 = 0x20;
const DIR_ATTR: u32 = 0x10;
const REPARSE: u32 = 0x400;
const T: i64 = 133_485_408_000_000_000; // 2024-01-01

/// A file reference.
pub fn r(record: u32, sequence: u16) -> FileRef {
    FileRef {
        record: u64::from(record),
        sequence,
    }
}

/// A volume with a bit of everything; see each record's comment.
pub fn sample() -> Volume {
    let mut v = Volume::new(64);
    v.sds = vec![(ALLOW, fake_sd(true)), (DENY, fake_sd(false))];
    v.system(ALLOW);
    let root = Volume::root();
    let mut put = |n: u32, rec: Vec<u8>| v.records[n as usize] = Some(rec);
    // 16: a tiny file, resident, with a DOS name too.
    put(
        16,
        record(
            16,
            1,
            1,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(root, 1, "A Long Name.txt"),
                file_name(root, 2, "ALONGN~1.TXT"),
                data_resident("", 5, b'x'),
            ],
        ),
    );
    // 17: docs/
    put(
        17,
        record(
            17,
            3,
            3,
            None,
            &[
                standard(DIR_ATTR, ALLOW, T, T),
                file_name(root, 1, "docs"),
                index_root(),
            ],
        ),
    );
    // 18: docs/big.bin, 100 000 bytes in 25 clusters.
    put(
        18,
        record(
            18,
            1,
            1,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(r(17, 3), 3, "big.bin"),
                non_resident(
                    0x80,
                    "",
                    0,
                    0,
                    24,
                    100_000,
                    25 * CLUSTER,
                    None,
                    &one_run(Some(40), 25),
                ),
            ],
        ),
    );
    // 19: a hard link: root/h1.bin and docs/h2.bin, 8192 bytes.
    put(
        19,
        record(
            19,
            1,
            1,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(root, 1, "h1.bin"),
                file_name(r(17, 3), 1, "h2.bin"),
                non_resident(0x80, "", 0, 0, 1, 8192, 8192, None, &one_run(Some(70), 2)),
            ],
        ),
    );
    // 20: a junction: listed, never entered.
    put(
        20,
        record(
            20,
            1,
            3,
            None,
            &[
                standard(DIR_ATTR | REPARSE, ALLOW, T, T),
                file_name(root, 1, "junction"),
                reparse(0xA000_0003),
                index_root(),
            ],
        ),
    );
    // 21: a cloud folder (entered) with 22: a placeholder, 1 MiB sparse with nothing allocated.
    put(
        21,
        record(
            21,
            1,
            3,
            None,
            &[
                standard(DIR_ATTR | REPARSE, ALLOW, T, T),
                file_name(root, 1, "cloud"),
                reparse(0x9000_001A),
                index_root(),
            ],
        ),
    );
    put(
        22,
        record(
            22,
            1,
            1,
            None,
            &[
                standard(FILE | 0x200 | REPARSE, ALLOW, T, T),
                file_name(r(21, 1), 1, "placeholder.txt"),
                reparse(0x9000_001A),
                non_resident(
                    0x80,
                    "",
                    0x8000,
                    0,
                    255,
                    1 << 20,
                    1 << 20,
                    Some(0),
                    &one_run(None, 256),
                ),
            ],
        ),
    );
    // 23: a WOF file: sparse, nothing allocated, its data in WofCompressedData (3 clusters).
    put(
        23,
        record(
            23,
            1,
            1,
            None,
            &[
                standard(FILE | 0x200 | REPARSE, ALLOW, T, T),
                file_name(root, 1, "wof.bin"),
                reparse(0x8000_0017),
                non_resident(
                    0x80,
                    "",
                    0x8000,
                    0,
                    48,
                    200_000,
                    49 * CLUSTER,
                    Some(0),
                    &one_run(None, 49),
                ),
                non_resident(
                    0x80,
                    "WofCompressedData",
                    0,
                    0,
                    2,
                    9000,
                    3 * CLUSTER,
                    None,
                    &one_run(Some(80), 3),
                ),
            ],
        ),
    );
    // 24: locked/ (denied) holding 25: locked/inner/ and 26: locked/inner/secret.bin (4096 bytes); and 27, a file
    // hard-linked into locked/ and docs/, which is the app's to count.
    put(
        24,
        record(
            24,
            1,
            3,
            None,
            &[
                standard(DIR_ATTR, DENY, T, T),
                file_name(root, 1, "locked"),
                index_root(),
            ],
        ),
    );
    put(
        25,
        record(
            25,
            1,
            3,
            None,
            &[
                standard(DIR_ATTR, ALLOW, T, T),
                file_name(r(24, 1), 1, "inner"),
                index_root(),
            ],
        ),
    );
    put(
        26,
        record(
            26,
            1,
            1,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(r(25, 1), 1, "secret.bin"),
                non_resident(0x80, "", 0, 0, 0, 4000, 4096, None, &one_run(Some(90), 1)),
            ],
        ),
    );
    put(
        27,
        record(
            27,
            1,
            1,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(r(24, 1), 1, "shared.bin"),
                file_name(r(17, 3), 1, "shared.bin"),
                data_resident("", 100, 1),
            ],
        ),
    );
    // 28: a file whose names and data are in 29, its extension record (as with an attribute list).
    put(28, record(28, 4, 1, None, &[standard(FILE, ALLOW, T, T)]));
    put(
        29,
        record(
            29,
            1,
            1,
            Some(r(28, 4)),
            &[
                file_name(root, 1, "extended.bin"),
                non_resident(0x80, "", 0, 0, 0, 10, CLUSTER, None, &one_run(Some(95), 1)),
            ],
        ),
    );
    // 30: deleted (not in use): never listed.
    put(
        30,
        record(
            30,
            2,
            0,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(root, 1, "deleted.bin"),
            ],
        ),
    );
    // 31: names a parent by an old sequence (the folder was deleted and its record reused): never listed.
    put(
        31,
        record(
            31,
            1,
            1,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(r(17, 2), 1, "orphan.bin"),
            ],
        ),
    );
    // 32: a file with an alternate stream: only its own data is the file's; the stream is a record's.
    put(
        32,
        record(
            32,
            1,
            1,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(root, 1, "streams.txt"),
                data_resident("", 20, 2),
                non_resident(
                    0x80,
                    "extra",
                    0,
                    0,
                    1,
                    7000,
                    2 * CLUSTER,
                    None,
                    &one_run(Some(100), 2),
                ),
            ],
        ),
    );
    // 33: under $Extend: a record of NTFS's.
    put(
        33,
        record(
            33,
            1,
            1,
            None,
            &[
                standard(FILE, ALLOW, T, T),
                file_name(r(11, 12), 1, "$UsnJrnl"),
                non_resident(
                    0x80,
                    "",
                    0,
                    0,
                    0,
                    100,
                    CLUSTER,
                    None,
                    &one_run(Some(110), 1),
                ),
            ],
        ),
    );
    v
}

/// The serial number of every test volume's boot sector.
pub const SERIAL: u64 = 0x1234_5678_9ABC_DEF0;

/// What the reader sends for an image, with [`FirstByte`] deciding which folders are locked.
pub fn stream(img: Vec<u8>) -> Vec<u8> {
    let mut access = FirstByte::default();
    let mut enc = crate::wire::Encoder::new(Vec::new());
    let _ = crate::serve(std::io::Cursor::new(img), &mut access, &mut enc, &|| false);
    enc.into_inner()
}
