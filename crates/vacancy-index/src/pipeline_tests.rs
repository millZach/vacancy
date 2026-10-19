//! The reader end to end on synthetic volumes: records in, stream out.

use crate::testkit::{self, CLUSTER, FirstByte, Volume, r, sample};
use crate::wire::{self, Decoder, ENTRY_DIR, ENTRY_ENTER, ENTRY_LOCKED, Frame};

const REPARSE: u32 = 0x400;

#[derive(Debug, Default)]
pub(crate) struct Listed {
    pub(crate) dirs: Vec<(u64, Vec<Item>)>,
    pub(crate) locked: Vec<wire::Locked>,
    pub(crate) records: Option<u64>,
    pub(crate) end: Option<wire::End>,
    pub(crate) error: Option<wire::ErrorCode>,
}

#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub(crate) name: String,
    pub(crate) frn: u64,
    pub(crate) flags: u8,
    pub(crate) logical: u64,
    pub(crate) allocated: u64,
    pub(crate) attributes: u32,
    pub(crate) tag: u32,
}

impl Listed {
    pub(crate) fn dir(&self, frn: u64) -> &[Item] {
        &self.dirs.iter().find(|d| d.0 == frn).expect("listed").1
    }
    pub(crate) fn item(&self, dir: u64, name: &str) -> &Item {
        self.dir(dir)
            .iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| panic!("{name} in {dir:#x}"))
    }
    pub(crate) fn has(&self, dir: u64, name: &str) -> bool {
        self.dir(dir).iter().any(|i| i.name == name)
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Listed {
    let mut out = Listed::default();
    let mut dec = Decoder::new(bytes);
    let mut name = Vec::new();
    while let Some(f) = dec.next_frame().expect("well-formed") {
        match f {
            Frame::Dir { frn } => out.dirs.push((frn, Vec::new())),
            Frame::Entries(es) => {
                for e in es {
                    let e = e.unwrap();
                    e.name_into(&mut name);
                    out.dirs.last_mut().unwrap().1.push(Item {
                        name: String::from_utf16_lossy(&name),
                        frn: e.frn,
                        flags: e.flags,
                        logical: e.logical,
                        allocated: e.allocated,
                        attributes: e.attributes,
                        tag: e.tag,
                    });
                }
            }
            Frame::DirEnd { frn, count } => {
                let last = out.dirs.last().unwrap();
                assert_eq!(last.0, frn);
                assert_eq!(last.1.len() as u64, count);
            }
            Frame::Locked(l) => out.locked.push(l),
            Frame::Records { bytes } => out.records = Some(bytes),
            Frame::End(e) => out.end = Some(e),
            Frame::Error(e) => out.error = Some(e),
            Frame::Header(_) | Frame::Progress(_) => {}
        }
    }
    out
}

pub(crate) fn serve_image(img: Vec<u8>) -> (Listed, FirstByte) {
    let mut access = FirstByte::default();
    let mut enc = wire::Encoder::new(Vec::new());
    let _ = crate::serve(std::io::Cursor::new(img), &mut access, &mut enc, &|| false);
    (decode(&enc.into_inner()), access)
}

#[test]
fn lists_a_volume_as_a_listing_would() {
    let (l, _) = serve_image(sample().image());
    assert_eq!(l.error, None);
    let root = Volume::root().raw();
    let docs = r(17, 3).raw();
    // The root lists user files only: none of NTFS's own, no DOS names, nothing deleted or orphaned.
    let names: Vec<&str> = l.dir(root).iter().map(|i| i.name.as_str()).collect();
    for hidden in [
        "$MFT",
        "$Secure",
        "$Extend",
        "ALONGN~1.TXT",
        "deleted.bin",
        "orphan.bin",
        ".",
    ] {
        assert!(!names.contains(&hidden), "{hidden} in {names:?}");
    }
    let tiny = l.item(root, "A Long Name.txt");
    assert_eq!(
        (tiny.logical, tiny.allocated),
        (5, 8),
        "resident: rounded to 8"
    );
    assert_eq!(tiny.frn, r(16, 1).raw());
    let big = l.item(docs, "big.bin");
    assert_eq!((big.logical, big.allocated), (100_000, 25 * CLUSTER));
    // Hard links: each name is listed, with the same id.
    assert_eq!(l.item(root, "h1.bin").frn, l.item(docs, "h2.bin").frn);
    // Links are listed, not entered; cloud folders are entered.
    let junction = l.item(root, "junction");
    assert_eq!(junction.flags, ENTRY_DIR);
    assert_eq!(junction.tag, 0xA000_0003);
    assert_eq!(l.item(root, "cloud").flags, ENTRY_DIR | ENTRY_ENTER);
    let ph = l.item(r(21, 1).raw(), "placeholder.txt");
    assert_eq!((ph.logical, ph.allocated), (1 << 20, 0));
    // WOF: a plain file whose allocation is its compressed data's.
    let wof = l.item(root, "wof.bin");
    assert_eq!(
        (wof.logical, wof.allocated, wof.tag),
        (200_000, 3 * CLUSTER, 0)
    );
    assert_eq!(wof.attributes & (REPARSE | 0x200), 0);
    // The extension record's name and data belong to its base record.
    let ext = l.item(root, "extended.bin");
    assert_eq!(
        (ext.frn, ext.logical, ext.allocated),
        (r(28, 4).raw(), 10, CLUSTER)
    );
    let streams = l.item(root, "streams.txt");
    assert_eq!((streams.logical, streams.allocated), (20, 24));
}

#[test]
fn locked_folders_are_totals_only() {
    let (l, access) = serve_image(sample().image());
    let root = Volume::root().raw();
    let locked = l.item(root, "locked");
    assert_eq!(locked.flags, ENTRY_DIR | ENTRY_LOCKED);
    // Nothing inside it is listed: not its folder, not its files.
    assert!(
        l.dirs
            .iter()
            .all(|d| d.0 != r(24, 1).raw() && d.0 != r(25, 1).raw())
    );
    assert_eq!(l.locked.len(), 1);
    let t = l.locked[0];
    assert_eq!(t.frn, r(24, 1).raw());
    assert_eq!(t.folders, 2, "locked and locked/inner");
    // secret.bin only; shared.bin also has a name in docs, so the app counts it there.
    assert_eq!((t.files, t.bytes), (1, 4096));
    assert!(l.has(r(17, 3).raw(), "shared.bin"));
    // Each security id is checked once.
    assert_eq!(access.asked, 2);
}

#[test]
fn a_root_the_account_cant_list_sends_nothing() {
    let mut v = sample();
    // The root's security id is ALLOW in the sample; point it at DENY.
    let root = Volume::root();
    v.records[5] = Some(testkit::record(
        5,
        root.sequence,
        3,
        None,
        &[
            testkit::standard(0x16, 0x101, 1, 1),
            testkit::file_name(root, 3, "."),
            testkit::index_root(),
        ],
    ));
    let (l, _) = serve_image(v.image());
    assert_eq!(l.error, Some(wire::ErrorCode::Access));
    assert!(l.dirs.is_empty(), "no name is sent");
}

#[test]
fn counts_what_ntfs_keeps_for_itself() {
    let v = sample();
    let mft = (v.records.len() * testkit::RECORD) as u64;
    let (l, _) = serve_image(v.image());
    let sds = CLUSTER; // one cluster of $SDS
    let usn = CLUSTER; // $Extend/$UsnJrnl
    let stream = 2 * CLUSTER; // streams.txt:extra
    assert_eq!(l.records, Some(mft + sds + usn + stream));
    let end = l.end.expect("an end");
    assert_eq!(end.locked, 1);
    assert_eq!(end.dirs, 3, "root, docs, cloud");
}

#[test]
fn a_torn_record_is_left_out_and_the_rest_read() {
    let mut v = sample();
    let mut torn = v.records[18].clone().unwrap();
    torn[510] ^= 0xFF; // the first sector's update sequence number
    v.records[18] = Some(torn);
    let (l, _) = serve_image(v.image());
    assert!(!l.has(r(17, 3).raw(), "big.bin"));
    assert!(l.has(r(17, 3).raw(), "h2.bin"));
}

#[test]
fn other_file_systems_and_locked_bitlocker_are_not_ntfs() {
    let mut img = sample().image();
    img[3..11].copy_from_slice(b"-FVE-FS-");
    let (l, _) = serve_image(img);
    assert_eq!(l.error, Some(wire::ErrorCode::NotNtfs));
    assert!(l.dirs.is_empty());
}

#[test]
fn a_damaged_index_is_unreadable_not_a_crash() {
    // $MFT's own record zeroed.
    let mut img = sample().image();
    let at = (4 * CLUSTER) as usize;
    img[at..at + testkit::RECORD].fill(0);
    let (l, _) = serve_image(img);
    assert_eq!(l.error, Some(wire::ErrorCode::Unreadable));
    // Every record but $MFT's is random bytes: no panic, and whatever is sent is well-formed.
    let v = sample();
    let mut img = v.image();
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mft = 4 * CLUSTER as usize;
    for byte in &mut img[mft + testkit::RECORD..mft + v.records.len() * testkit::RECORD] {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *byte = x as u8;
    }
    let _ = serve_image(img);
}

#[test]
fn records_that_flip_bits_never_panic() {
    // Each byte of a few records, flipped in turn: the reader may leave the record out, never crash.
    let v = sample();
    for n in [16usize, 19, 23, 29] {
        let rec = v.records[n].clone().unwrap();
        for i in (0..rec.len()).step_by(3) {
            let mut v2 = sample();
            let mut bad = rec.clone();
            bad[i] ^= 0xA5;
            v2.records[n] = Some(bad);
            let _ = serve_image(v2.image());
        }
    }
}

/// A volume whose disk fails part way through the index (a bad sector, a drive unplugged): the read ends with an
/// error, sent, instead of the reading thread waiting for ever.
#[test]
fn a_disk_that_fails_mid_index_ends_the_read() {
    struct Failing {
        img: std::io::Cursor<Vec<u8>>,
        from: u64,
    }
    impl crate::Disk for Failing {
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
            if offset >= self.from {
                return Err(std::io::Error::other("a bad sector"));
            }
            crate::Disk::read_at(&mut self.img, offset, buf)
        }
    }
    let img = sample().image();
    // The MFT starts at cluster 4; fail from its third chunk of 8 records on.
    let from = 4 * CLUSTER + 16 * testkit::RECORD as u64;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut access = FirstByte::default();
        let mut enc = wire::Encoder::new(Vec::new());
        let disk = Failing {
            img: std::io::Cursor::new(img),
            from,
        };
        let r = crate::serve(disk, &mut access, &mut enc, &|| false);
        let _ = tx.send((r.is_err(), decode(&enc.into_inner()).error));
    });
    let (failed, error) = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("the read ends");
    assert!(failed);
    assert_eq!(error, Some(wire::ErrorCode::Unreadable));
}
