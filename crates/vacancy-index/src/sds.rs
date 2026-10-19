//! `$Secure:$SDS`: every security descriptor on the volume, each once, found by the security id a file's
//! `$STANDARD_INFORMATION` holds.
//!
//! The stream is made of 256 KiB blocks, each followed by a mirror copy. A block holds entries one after another,
//! each 16-byte aligned: a hash (`u32`), the security id (`u32`), the entry's own offset in the stream (`u64`), its
//! length with this 20-byte header (`u32`), then the self-relative security descriptor.

use std::collections::HashMap;
use std::ops::Range;

use crate::le::{u32_at, u64_at};

const BLOCK: usize = 256 * 1024;
const HEADER: usize = 20;
/// No descriptor is anywhere near this big (Windows caps them at 64 KiB).
const LONGEST: usize = 64 * 1024 + HEADER;

/// Where each security id's descriptor is in the stream's bytes.
pub fn index(sds: &[u8]) -> HashMap<u32, Range<usize>> {
    let mut out = HashMap::new();
    let mut block = 0usize;
    while block < sds.len() {
        let end = (block + BLOCK).min(sds.len());
        let mut at = block;
        while at + HEADER <= end {
            let (Some(id), Some(offset), Some(len)) = (
                u32_at(sds, at + 4),
                u64_at(sds, at + 8),
                u32_at(sds, at + 16).map(|l| l as usize),
            ) else {
                break;
            };
            if offset != at as u64 || !(HEADER + 20..=LONGEST).contains(&len) || at + len > end {
                break;
            }
            out.entry(id).or_insert(at + HEADER..at + len);
            at = (at + len + 15) & !15;
        }
        // The next primary block: skip this one's mirror.
        block += 2 * BLOCK;
    }
    out
}

#[cfg(any(test, feature = "testkit"))]
pub(crate) fn entry(at: usize, id: u32, sd: &[u8]) -> Vec<u8> {
    let mut e = Vec::new();
    e.extend_from_slice(&0xDEADu32.to_le_bytes());
    e.extend_from_slice(&id.to_le_bytes());
    e.extend_from_slice(&(at as u64).to_le_bytes());
    e.extend_from_slice(&((HEADER + sd.len()) as u32).to_le_bytes());
    e.extend_from_slice(sd);
    while !e.len().is_multiple_of(16) {
        e.push(0);
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_descriptors_and_skips_the_mirror() {
        let sd_a = [1u8; 40];
        let sd_b = [2u8; 60];
        let mut s = entry(0, 0x100, &sd_a);
        let at_b = s.len();
        s.extend(entry(at_b, 0x101, &sd_b));
        s.resize(BLOCK, 0);
        // The mirror block says something else; it must not be read.
        s.extend(entry(BLOCK, 0x999, &sd_a));
        s.resize(2 * BLOCK, 0);
        s.extend(entry(2 * BLOCK, 0x102, &sd_b));
        let ix = index(&s);
        assert_eq!(ix.len(), 3);
        assert_eq!(&s[ix[&0x100].clone()], &sd_a);
        assert_eq!(&s[ix[&0x101].clone()], &sd_b);
        assert!(ix.contains_key(&0x102));
        assert!(!ix.contains_key(&0x999));
    }

    #[test]
    fn stops_at_entries_that_dont_add_up() {
        let mut s = entry(0, 1, &[1u8; 40]);
        let at = s.len();
        let mut bad = entry(at + 16, 2, &[1u8; 40]); // claims another offset
        s.append(&mut bad);
        assert_eq!(index(&s).len(), 1);
        let mut long = entry(0, 3, &[1u8; 40]);
        long[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(index(&long).is_empty());
        assert!(index(&[0u8; 10]).is_empty());
    }
}
