//! The reader on a real NTFS volume written by another implementation (ntfs-3g; see `fixtures/make-image.sh`):
//! every name a listing would show, with the record, kind and sizes `fixtures/small.expected.tsv` gives, and nothing
//! a listing wouldn't show. It covers what hand-made records can't vouch for: real fixups, an `$MFT` in two pieces,
//! a file with 31 names whose record overflowed into 15 extension records (an attribute list), a folder index too
//! big for its record, compressed and sparse data, alternate streams, reparse points, DOS 8.3 names and a deleted
//! file.

use std::collections::HashMap;
use std::io::Read;

use vacancy_index::wire::{Decoder, ENTRY_DIR, ENTRY_ENTER, ENTRY_LOCKED, Encoder, Frame};
use vacancy_index::{Access, Security};

const MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

struct Everyone;

impl Access for Everyone {
    fn can_list(&mut self, security: Security<'_>) -> bool {
        matches!(security, Security::Descriptor(_))
    }
}

#[derive(Debug, Clone)]
struct Seen {
    record: u64,
    flags: u8,
    attributes: u32,
    tag: u32,
    logical: u64,
    allocated: u64,
}

fn image() -> Vec<u8> {
    let gz = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/small.ntfs.gz"
    ))
    .expect("the fixture image");
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&gz[..])
        .read_to_end(&mut out)
        .expect("gunzip");
    out
}

/// Every name the reader sent, by path from the root, and the records total.
fn listing() -> (HashMap<String, Seen>, u64) {
    let mut enc = Encoder::new(Vec::new());
    vacancy_index::serve(
        std::io::Cursor::new(image()),
        &mut Everyone,
        &mut enc,
        &|| false,
    )
    .expect("the image reads")
    .expect("not stopped");
    let bytes = enc.into_inner();
    let mut dec = Decoder::new(&bytes[..]);
    let mut paths: HashMap<u64, String> = HashMap::new();
    let mut out = HashMap::new();
    let mut records = 0;
    let mut current = String::new();
    let mut name = Vec::new();
    while let Some(f) = dec.next_frame().expect("well-formed") {
        match f {
            Frame::Dir { frn } => {
                current = if frn & MASK == 5 {
                    String::new()
                } else {
                    paths.get(&frn).cloned().expect("announced before listed")
                };
            }
            Frame::Entries(es) => {
                for e in es {
                    let e = e.unwrap();
                    e.name_into(&mut name);
                    let n = String::from_utf16(&name).expect("valid UTF-16");
                    let path = if current.is_empty() {
                        n
                    } else {
                        format!("{current}/{n}")
                    };
                    if e.flags & ENTRY_ENTER != 0 {
                        paths.insert(e.frn, path.clone());
                    }
                    let previous = out.insert(
                        path.clone(),
                        Seen {
                            record: e.frn & MASK,
                            flags: e.flags,
                            attributes: e.attributes,
                            tag: e.tag,
                            logical: e.logical,
                            allocated: e.allocated,
                        },
                    );
                    assert!(previous.is_none(), "{path} listed twice");
                }
            }
            Frame::Records { bytes } => records = bytes,
            Frame::Error(e) => panic!("the reader failed: {e:?}"),
            _ => {}
        }
    }
    (out, records)
}

#[test]
fn every_name_a_listing_shows_and_nothing_else() {
    let (seen, records) = listing();
    let expected = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/small.expected.tsv"
    ))
    .unwrap();
    let mut rows = 0;
    for line in expected.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        let [path, record, kind, logical, allocated] = f[..] else {
            panic!("bad row {line}");
        };
        rows += 1;
        let s = seen
            .get(path)
            .unwrap_or_else(|| panic!("{path} is missing from the listing"));
        assert_eq!(s.record, record.parse::<u64>().unwrap(), "{path}: record");
        let flags = match kind {
            "dir" | "clouddir" => ENTRY_DIR | ENTRY_ENTER,
            "dirlink" => ENTRY_DIR,
            _ => 0,
        };
        assert_eq!(s.flags, flags, "{path}: kind {kind}");
        if kind == "file" {
            assert_eq!(s.logical, logical.parse::<u64>().unwrap(), "{path}: length");
            assert_eq!(
                s.allocated,
                allocated.parse::<u64>().unwrap(),
                "{path}: allocation"
            );
        }
        if kind == "dirlink" {
            assert_eq!(s.tag, 0xA000_0003, "{path}: the junction's tag");
        }
        if kind == "filelink" {
            assert_eq!(s.tag & 0x2000_0000, 0x2000_0000, "{path}: a name surrogate");
        }
    }
    // bigdir's 3,000 one-byte files, each kept in its record (8 bytes allocated, as Windows lists it).
    for i in 0..3000 {
        let path = format!("bigdir/f{i:05}.txt");
        let s = seen.get(&path).unwrap_or_else(|| panic!("{path}"));
        assert_eq!((s.logical, s.allocated, s.flags), (1, 8, 0), "{path}");
    }
    assert_eq!(seen.len(), rows + 3000, "nothing a listing wouldn't show");
    // What listings never show: DOS names, deleted files, NTFS's own files.
    for hidden in [
        "PROGRA~1",
        "deleted.bin",
        "$MFT",
        "$Secure",
        "$Extend",
        "$LogFile",
    ] {
        assert!(!seen.contains_key(hidden), "{hidden}");
    }
    // The file with 31 names: each one, all the same record, whose names and data came from extension records.
    let target: Vec<&Seen> = seen
        .iter()
        .filter(|(p, _)| p.starts_with("manylinks/"))
        .map(|(_, s)| s)
        .collect();
    assert_eq!(target.len(), 31);
    assert!(target.iter().all(|s| s.record == 78 && s.logical == 8000));
    // WOF: a plain file to a listing.
    let wof = &seen["wof.bin"];
    assert_eq!((wof.tag, wof.attributes & 0x400), (0, 0));
    assert!(seen.values().all(|s| s.flags & ENTRY_LOCKED == 0));
    // NTFS's records: at least the MFT itself (3,145 records of 1 KiB, in whole clusters).
    assert!(records >= 3145 * 1024, "records {records}");
}
