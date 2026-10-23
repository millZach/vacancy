//! Fast scan's snapshots from the reader's stream: a real stream from a synthetic volume, and hostile ones.

use std::path::PathBuf;

use vacancy_index::testkit::{self, CLUSTER, SERIAL, r, sample};
use vacancy_index::wire::{
    ENTRY_DIR, ENTRY_ENTER, ENTRY_LOCKED, Encoder, End, ErrorCode, Header, Locked, OutEntry,
};

use super::*;
use crate::snapshot::DirFlags;

const CAPACITY: u64 = 1 << 30;

fn expect() -> Expect {
    Expect {
        root: PathBuf::from("vol"),
        volume_serial: SERIAL,
        subtree: None,
        capacity: sample().image().len() as u64,
    }
}

fn read(bytes: &[u8], expect: &Expect) -> Result<Snapshot, FastError> {
    snapshot_from_stream(
        bytes,
        expect,
        &ScanOptions {
            keep_files_from: 4096,
            ..ScanOptions::default()
        },
        &mut |_| {},
        &mut |_| {},
        &CancelToken::new(),
    )
}

/// The folder `name` at the top of the volume (platform-neutral paths, so this runs on macOS too).
fn find(snap: &Snapshot, name: &str) -> DirId {
    snap.find(&PathBuf::from("vol").join(name))
        .unwrap_or_else(|| panic!("{name} in the snapshot"))
}

#[test]
fn a_volume_reads_into_a_snapshot_like_a_scans() {
    let bytes = testkit::stream(sample().image());
    let snap = read(&bytes, &expect()).expect("a snapshot");
    let root = snap.roots()[0].dir;
    assert_eq!(snap.roots()[0].volume, Some(SERIAL));
    // Files: tiny (8), big.bin (25 clusters), the hard link once (2 clusters), placeholder (0), wof (3 clusters),
    // shared.bin (104, counted here: it has a name in docs), extended.bin (1 cluster), streams.txt (24).
    let expected = 8 + 25 * CLUSTER + 2 * CLUSTER + 3 * CLUSTER + 104 + CLUSTER + 24;
    assert_eq!(snap.tree(root).bytes, expected);
    assert_eq!(
        snap.stats().hard_links_merged,
        1,
        "h2.bin: h1.bin was counted"
    );
    let docs = find(&snap, "docs");
    assert_eq!(snap.identity(docs), Some(u128::from(r(17, 3).raw())));
    let big = snap
        .files(docs)
        .find(|&f| snap.file(f).name == "big.bin")
        .expect("big.bin is kept");
    assert_eq!(snap.file_identity(big), Some(u128::from(r(18, 1).raw())));
    assert_eq!(snap.file_logical(big), 100_000);
    // The junction is recorded, not entered; the cloud folder is entered.
    let junction = find(&snap, "junction");
    assert!(snap.flags(junction).contains(DirFlags::LINK));
    let cloud = find(&snap, "cloud");
    assert!(snap.flags(cloud).contains(DirFlags::CLOUD));
    assert_eq!(snap.own(cloud).files, 1);
    // The locked folder: there, unreadable as in a normal scan, empty, and its totals kept apart.
    let locked = find(&snap, "locked");
    assert!(snap.flags(locked).contains(DirFlags::UNREADABLE));
    assert_eq!(snap.children(locked).len(), 0);
    assert_eq!(snap.files(locked).len(), 0);
    let fast = snap.fast().expect("fast facts");
    assert_eq!(
        fast.locked,
        vec![crate::snapshot::LockedFolder {
            dir: locked,
            bytes: 4096,
            files: 1,
            folders: 2
        }]
    );
    assert!(fast.records_bytes > 0);
    // Digests are made as the normal scan makes them: every entry of a folder, by its id and sizes.
    let d = snap.digests(docs);
    let mut all = 0u64;
    let mut files = 0u64;
    for (id, logical, allocated) in [
        (r(18, 1).raw(), 100_000u64, 25 * CLUSTER),
        (r(19, 1).raw(), 8192, 8192),
        (r(27, 1).raw(), 100, 104),
    ] {
        let k = crate::snapshot::file_digest_key(u128::from(id), logical, allocated);
        all = all.wrapping_add(k);
        files = files.wrapping_add(k);
    }
    assert_eq!((d.all, d.files), (all, files));
}

#[test]
fn fixture_mode_keeps_only_the_fixtures_folder() {
    let bytes = testkit::stream(sample().image());
    let mut e = expect();
    e.root = PathBuf::from("fixture");
    e.subtree = Some(r(17, 3).raw());
    let snap = read(&bytes, &e).expect("a snapshot");
    assert_eq!(snap.roots().len(), 1);
    let root = snap.roots()[0].dir;
    assert_eq!(snap.path(root), PathBuf::from("fixture"));
    assert_eq!(snap.tree(root).files, 3, "big.bin, h2.bin, shared.bin");
    assert_eq!(snap.dir_count(), 1);
    let fast = snap.fast().unwrap();
    assert!(fast.locked.is_empty(), "the locked folder is outside it");
    assert_eq!(
        fast.records_bytes, 0,
        "the volume's records aren't the fixture's"
    );
}

#[test]
fn a_fixture_folder_missing_from_the_index_is_an_error() {
    let bytes = testkit::stream(sample().image());
    let mut e = expect();
    e.subtree = Some(r(60, 1).raw());
    assert!(matches!(read(&bytes, &e), Err(FastError::Unreadable(_))));
}

#[test]
fn another_drives_index_is_refused() {
    let bytes = testkit::stream(sample().image());
    let mut e = expect();
    e.volume_serial = 7;
    assert!(matches!(read(&bytes, &e), Err(FastError::Unreadable(_))));
}

#[test]
fn the_readers_errors_are_reported() {
    let mut img = sample().image();
    img[3..11].copy_from_slice(b"-FVE-FS-");
    let bytes = testkit::stream(img);
    assert!(matches!(read(&bytes, &expect()), Err(FastError::NotNtfs)));
}

/// A hand-made stream: a header, then frames from `body`.
fn stream(body: impl FnOnce(&mut Encoder<Vec<u8>>)) -> Vec<u8> {
    let mut enc = Encoder::new(Vec::new());
    enc.header(&Header {
        volume_serial: SERIAL,
        cluster_size: 4096,
        record_size: 1024,
        records: 1000,
        total_clusters: CAPACITY / 4096,
    })
    .unwrap();
    body(&mut enc);
    enc.into_inner()
}

fn big() -> Expect {
    Expect {
        capacity: CAPACITY,
        ..expect()
    }
}

fn entry<'a>(frn: u64, name: &'a [u16], flags: u8, allocated: u64) -> OutEntry<'a> {
    let dir = flags & ENTRY_DIR != 0;
    OutEntry {
        frn,
        attributes: if dir { 0x10 } else { 0x20 },
        tag: 0,
        logical: allocated,
        allocated,
        modified: 0,
        accessed: 0,
        flags,
        name,
    }
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

const ROOT: u64 = 5 | (5 << 48);

/// Root holding `a/` (entered) holding `f.bin`.
fn good(enc: &mut Encoder<Vec<u8>>) {
    let a = units("a");
    let f = units("f.bin");
    enc.dir(ROOT).unwrap();
    enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_ENTER, 0))
        .unwrap();
    enc.dir_end(ROOT, 1).unwrap();
    enc.dir(20).unwrap();
    enc.entry(&entry(21, &f, 0, 4096)).unwrap();
    enc.dir_end(20, 1).unwrap();
    enc.records(0).unwrap();
    enc.end(&End {
        dirs: 2,
        entries: 2,
        locked: 0,
    })
    .unwrap();
}

#[test]
fn a_well_formed_hand_made_stream_reads() {
    let snap = read(&stream(good), &big()).expect("reads");
    assert_eq!(snap.stats().files, 1);
    assert_eq!(snap.dir_count(), 2);
}

fn refused(body: impl FnOnce(&mut Encoder<Vec<u8>>)) {
    let r = read(&stream(body), &big());
    assert!(
        matches!(r, Err(FastError::Unreadable(_))),
        "refused: {:?}",
        r.map(|s| s.stats())
    );
}

#[test]
fn hostile_streams_fail_closed() {
    let a = units("a");
    let f = units("f.bin");
    // A folder listed that nobody announced.
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.dir_end(ROOT, 0).unwrap();
        enc.dir(99).unwrap();
        enc.dir_end(99, 0).unwrap();
    });
    // Listed twice.
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_ENTER, 0))
            .unwrap();
        enc.dir_end(ROOT, 1).unwrap();
        enc.dir(20).unwrap();
        enc.dir_end(20, 0).unwrap();
        enc.dir(20).unwrap();
        enc.dir_end(20, 0).unwrap();
    });
    // A cycle: a folder announcing the root, or announcing itself.
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_ENTER, 0))
            .unwrap();
        enc.dir_end(ROOT, 1).unwrap();
        enc.dir(20).unwrap();
        enc.entry(&entry(ROOT, &a, ENTRY_DIR | ENTRY_ENTER, 0))
            .unwrap();
        enc.dir_end(20, 1).unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_ENTER, 0))
            .unwrap();
        enc.entry(&entry(20, &f, ENTRY_DIR | ENTRY_ENTER, 0))
            .unwrap();
        enc.dir_end(ROOT, 2).unwrap();
    });
    // Names that would change a path.
    for bad in ["..", ".", "a\\b", "a/b", "x\0y"] {
        let n = units(bad);
        refused(|enc| {
            enc.dir(ROOT).unwrap();
            enc.entry(&entry(21, &n, 0, 0)).unwrap();
            enc.dir_end(ROOT, 1).unwrap();
        });
    }
    // A link marked as entered; a folder neither entered nor locked; a file marked a folder.
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        let mut e = entry(20, &a, ENTRY_DIR | ENTRY_ENTER, 0);
        e.attributes = 0x10 | 0x400;
        e.tag = 0xA000_0003;
        enc.entry(&e).unwrap();
        enc.dir_end(ROOT, 1).unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(20, &a, ENTRY_DIR, 0)).unwrap();
        enc.dir_end(ROOT, 1).unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        let mut e = entry(21, &f, ENTRY_DIR | ENTRY_ENTER, 0);
        e.attributes = 0x20;
        enc.entry(&e).unwrap();
        enc.dir_end(ROOT, 1).unwrap();
    });
    // Bigger than the drive: one file, or all of them together.
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(21, &f, 0, CAPACITY + 1)).unwrap();
        enc.dir_end(ROOT, 1).unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        for i in 0..3u64 {
            enc.entry(&entry(100 + i, &units(&format!("f{i}")), 0, CAPACITY / 2))
                .unwrap();
        }
        enc.dir_end(ROOT, 3).unwrap();
    });
    // The end before everything is listed; no end at all; a miscount.
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_ENTER, 0))
            .unwrap();
        enc.dir_end(ROOT, 1).unwrap();
        enc.end(&End {
            dirs: 1,
            entries: 1,
            locked: 0,
        })
        .unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.dir_end(ROOT, 0).unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(21, &f, 0, 0)).unwrap();
        enc.dir_end(ROOT, 2).unwrap();
    });
    // Locked totals for a folder that isn't locked; a locked folder whose totals never come.
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_LOCKED, 0))
            .unwrap();
        enc.dir_end(ROOT, 1).unwrap();
        enc.end(&End {
            dirs: 1,
            entries: 1,
            locked: 1,
        })
        .unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.dir_end(ROOT, 0).unwrap();
        enc.locked(&Locked {
            frn: 20,
            bytes: 1,
            files: 1,
            folders: 1,
        })
        .unwrap();
    });
    // Not starting at the root; an entry naming one of NTFS's own records.
    refused(|enc| {
        enc.dir(20).unwrap();
        enc.dir_end(20, 0).unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(3, &f, 0, 0)).unwrap();
        enc.dir_end(ROOT, 1).unwrap();
    });
}

/// The good stream, then `good` again: the second header and everything after it is refused.
#[test]
fn trailing_and_repeated_frames_are_refused() {
    let mut bytes = stream(good);
    bytes.extend(stream(good));
    assert!(read(&bytes, &big()).is_err());
    let mut bytes = stream(good);
    let mut enc = Encoder::new(Vec::new());
    enc.error(ErrorCode::Unreadable).unwrap();
    bytes.extend(enc.into_inner());
    assert!(read(&bytes, &big()).is_err());
}

/// Every byte of a real stream changed in turn, and random cuts: never a panic, and anything accepted is a tree.
#[test]
fn corrupted_streams_never_panic() {
    let bytes = testkit::stream(sample().image());
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    for i in 0..bytes.len() {
        let mut b = bytes.clone();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        b[i] ^= (x as u8) | 1;
        if let Ok(snap) = read(&b, &expect()) {
            // Whatever is accepted is still one tree whose folders all lead to the root.
            for d in snap.dirs() {
                let _ = snap.path(d);
            }
        }
    }
    for cut in (0..bytes.len()).step_by(7) {
        assert!(read(&bytes[..cut], &expect()).is_err(), "cut at {cut}");
    }
}

#[test]
fn a_cancel_stops_the_read() {
    let bytes = testkit::stream(sample().image());
    let cancel = CancelToken::new();
    cancel.cancel();
    let r = snapshot_from_stream(
        &bytes[..],
        &expect(),
        &ScanOptions::default(),
        &mut |_| {},
        &mut |_| {},
        &cancel,
    );
    assert!(matches!(r, Err(FastError::Cancelled)));
}

/// Fixture mode with the fixture folder locked to the account: refused, so the normal scan runs, never an empty map.
#[test]
fn a_locked_fixture_folder_is_refused_not_mapped_empty() {
    let a = units("fixture");
    let bytes = stream(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_LOCKED, 0))
            .unwrap();
        enc.dir_end(ROOT, 1).unwrap();
        enc.locked(&Locked {
            frn: 20,
            bytes: 10,
            files: 1,
            folders: 1,
        })
        .unwrap();
        enc.records(0).unwrap();
        enc.end(&End {
            dirs: 1,
            entries: 1,
            locked: 1,
        })
        .unwrap();
    });
    let mut e = big();
    e.subtree = Some(20);
    assert!(matches!(read(&bytes, &e), Err(FastError::Unreadable(_))));
    // The same stream for the whole drive is fine: the locked folder is one block among others.
    let snap = read(&bytes, &big()).expect("reads");
    assert_eq!(snap.fast().unwrap().locked.len(), 1);
}

/// Frames out of their place: progress once folders are being listed, records inside a folder, an end that
/// miscounts the locked folders.
#[test]
fn frames_out_of_place_are_refused() {
    let a = units("a");
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.progress(&vacancy_index::wire::Progress::default())
            .unwrap();
        enc.dir_end(ROOT, 0).unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.records(0).unwrap();
        enc.dir_end(ROOT, 0).unwrap();
    });
    refused(|enc| {
        enc.dir(ROOT).unwrap();
        enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_LOCKED, 0))
            .unwrap();
        enc.dir_end(ROOT, 1).unwrap();
        enc.locked(&Locked {
            frn: 20,
            bytes: 1,
            files: 1,
            folders: 1,
        })
        .unwrap();
        enc.end(&End {
            dirs: 1,
            entries: 1,
            locked: 0,
        })
        .unwrap();
    });
}

/// A backup of a browser profile made with hard links: `Preferences` and `History` are the same two records in
/// `a` and `b`. Both folders get the markers, whichever is listed first; the bytes count once.
#[test]
fn hard_linked_markers_mark_both_folders_in_either_order() {
    let a = units("a");
    let b = units("b");
    let prefs = units("Preferences");
    let history = units("History");
    for a_first in [true, false] {
        let bytes = stream(|enc| {
            enc.dir(ROOT).unwrap();
            enc.entry(&entry(20, &a, ENTRY_DIR | ENTRY_ENTER, 0))
                .unwrap();
            enc.entry(&entry(22, &b, ENTRY_DIR | ENTRY_ENTER, 0))
                .unwrap();
            enc.dir_end(ROOT, 2).unwrap();
            let order = if a_first { [20, 22] } else { [22, 20] };
            for dir in order {
                enc.dir(dir).unwrap();
                enc.entry(&entry(30, &prefs, 0, 4096)).unwrap();
                enc.entry(&entry(31, &history, 0, 4096)).unwrap();
                enc.dir_end(dir, 2).unwrap();
            }
            enc.records(0).unwrap();
            enc.end(&End {
                dirs: 3,
                entries: 6,
                locked: 0,
            })
            .unwrap();
        });
        let snap = read(&bytes, &big()).expect("reads");
        assert_eq!(snap.stats().hard_links_merged, 2, "a first: {a_first}");
        assert_eq!(snap.tree(snap.roots()[0].dir).bytes, 2 * 4096);
        for name in ["a", "b"] {
            let m = snap.markers(find(&snap, name));
            assert!(
                m.contains(crate::snapshot::Markers::PREFERENCES)
                    && m.contains(crate::snapshot::Markers::CHROMIUM_STORE),
                "{name} is a profile (a first: {a_first})"
            );
        }
    }
}
