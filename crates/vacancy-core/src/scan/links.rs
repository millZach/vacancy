//! Counting hard-linked files once.
//!
//! A hard-linked file has several names, often in different folders (Windows links most of `System32` into
//! `WinSxS`). Each name shows up in a listing with the file's full size, so a naive total counts it once per name.
//! The scanner instead counts a file the first time it sees it and skips later names.
//!
//! On NTFS every file id is a record number in the Master File Table, a dense range from zero, so "seen" is one bit
//! per record: about half a megabyte for a drive of four million files, whatever their links. Elsewhere (macOS,
//! ReFS) ids are sparse, so seen files go in a hash set; macOS listings carry the link count, so only files that
//! really have several names are put there.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// How a file is identified across its names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LinkKey {
    /// A dense index, such as an NTFS file record number.
    #[cfg_attr(unix, allow(dead_code, reason = "only NTFS has dense file ids"))]
    Index(u64),
    /// Any other id, unique on the scanned volumes.
    Id(u128),
}

/// Records per bitmap chunk, and chunks: together every NTFS record number (NTFS allows at most 2^32 files).
const CHUNK_BITS: u64 = 1 << 16;
const CHUNKS: u64 = 1 << 16;
const WORDS_PER_CHUNK: usize = (CHUNK_BITS / 64) as usize;
const SHARDS: usize = 16;

/// The files seen so far. Shared by all scan workers; lock-free for dense indexes.
pub(crate) struct LinkSet {
    chunks: Box<[OnceLock<Box<[AtomicU64]>>]>,
    sets: [Mutex<HashSet<u128>>; SHARDS],
}

impl LinkSet {
    pub(crate) fn new() -> Self {
        Self {
            chunks: (0..CHUNKS).map(|_| OnceLock::new()).collect(),
            sets: std::array::from_fn(|_| Mutex::new(HashSet::new())),
        }
    }

    /// True the first time a file is seen, false for each later name of the same file.
    pub(crate) fn first_sighting(&self, key: LinkKey) -> bool {
        match key {
            LinkKey::Index(i) if i < CHUNK_BITS * CHUNKS => {
                let chunk = self.chunks[(i / CHUNK_BITS) as usize]
                    .get_or_init(|| (0..WORDS_PER_CHUNK).map(|_| AtomicU64::new(0)).collect());
                let bit = i % CHUNK_BITS;
                let mask = 1u64 << (bit % 64);
                chunk[(bit / 64) as usize].fetch_or(mask, Ordering::Relaxed) & mask == 0
            }
            LinkKey::Index(i) => self.insert(u128::from(i)),
            LinkKey::Id(id) => self.insert(id),
        }
    }

    fn insert(&self, id: u128) -> bool {
        let shard = (id ^ (id >> 64)) as usize % SHARDS;
        self.sets[shard]
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id)
    }
}

impl std::fmt::Debug for LinkSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkSet").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_each_file_once() {
        let set = LinkSet::new();
        for key in [
            LinkKey::Index(0),
            LinkKey::Index(70_000),
            LinkKey::Index(u64::MAX),
            LinkKey::Id(42),
        ] {
            assert!(set.first_sighting(key), "{key:?} is new");
            assert!(!set.first_sighting(key), "{key:?} was seen");
        }
        assert!(set.first_sighting(LinkKey::Index(1)), "a neighbour is new");
        assert!(set.first_sighting(LinkKey::Id(43)));
    }
}
