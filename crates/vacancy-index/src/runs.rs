//! Data runs ("mapping pairs"): where a non-resident attribute's clusters are on the volume.
//!
//! Each run starts with a header byte: its low nibble is how many bytes give the run's length in clusters, its high
//! nibble how many give the run's start, as a signed offset from the previous run's start. A start of zero bytes
//! is a sparse run (no clusters on disk). A zero header ends the list.

/// One run: `clusters` clusters starting at cluster `lcn`, or a hole when `lcn` is `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub lcn: Option<u64>,
    pub clusters: u64,
}

/// Decodes the runs in `b`, which starts at an attribute's mapping pairs. `None` if they are malformed: a field
/// wider than eight bytes, a run of no clusters, a start before the volume or past `total_clusters`, more VCNs
/// than `expected_clusters`, or no terminating zero.
pub fn decode(b: &[u8], total_clusters: u64, expected_clusters: u64) -> Option<Vec<Run>> {
    let mut runs = Vec::new();
    let mut at = 0usize;
    let mut lcn: i64 = 0;
    let mut vcns: u64 = 0;
    loop {
        let header = *b.get(at)?;
        if header == 0 {
            return (vcns == expected_clusters).then_some(runs);
        }
        let len_size = usize::from(header & 0x0F);
        let off_size = usize::from(header >> 4);
        if len_size == 0 || len_size > 8 || off_size > 8 {
            return None;
        }
        let len_bytes = b.get(at + 1..at + 1 + len_size)?;
        let clusters = unsigned(len_bytes);
        if clusters == 0 {
            return None;
        }
        vcns = vcns.checked_add(clusters)?;
        if vcns > expected_clusters {
            return None;
        }
        let start = at + 1 + len_size;
        let run_lcn = if off_size == 0 {
            None
        } else {
            let delta = signed(b.get(start..start + off_size)?);
            lcn = lcn.checked_add(delta)?;
            let first = u64::try_from(lcn).ok()?;
            if first.checked_add(clusters)? > total_clusters {
                return None;
            }
            Some(first)
        };
        runs.push(Run {
            lcn: run_lcn,
            clusters,
        });
        at = start + off_size;
    }
}

fn unsigned(b: &[u8]) -> u64 {
    b.iter()
        .rev()
        .fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte))
}

fn signed(b: &[u8]) -> i64 {
    let raw = unsigned(b);
    let bits = b.len() * 8;
    if bits == 64 || bits == 0 {
        raw as i64
    } else if raw & (1 << (bits - 1)) != 0 {
        (raw | (u64::MAX << bits)) as i64
    } else {
        raw as i64
    }
}

/// Clusters the runs take on disk (holes take none).
pub fn allocated_clusters(runs: &[Run]) -> u64 {
    runs.iter()
        .filter(|r| r.lcn.is_some())
        .map(|r| r.clusters)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_runs_holes_and_backward_offsets() {
        // 0x18 clusters at 0x5634; a hole of 4; 0x10 clusters at 0x5634 - 0x34 = 0x5600.
        let b = [0x21, 0x18, 0x34, 0x56, 0x01, 0x04, 0x11, 0x10, 0xCC, 0x00];
        let runs = decode(&b, 1 << 20, 0x18 + 4 + 0x10).unwrap();
        assert_eq!(
            runs,
            vec![
                Run {
                    lcn: Some(0x5634),
                    clusters: 0x18
                },
                Run {
                    lcn: None,
                    clusters: 4
                },
                Run {
                    lcn: Some(0x5634 - 0x34),
                    clusters: 0x10
                },
            ]
        );
        assert_eq!(allocated_clusters(&runs), 0x28);
    }

    #[test]
    fn refuses_malformed_runs() {
        let ok = [0x21, 0x18, 0x34, 0x56, 0x00];
        assert!(decode(&ok, 1 << 20, 0x18).is_some());
        assert!(
            decode(&ok, 1 << 20, 0x19).is_none(),
            "fewer clusters than the attribute says"
        );
        assert!(
            decode(&ok, 1 << 20, 0x17).is_none(),
            "more clusters than the attribute says"
        );
        assert!(
            decode(&ok, 0x5000, 0x18).is_none(),
            "past the end of the volume"
        );
        assert!(decode(&ok[..4], 1 << 20, 0x18).is_none(), "no terminator");
        assert!(
            decode(&[0x09, 0, 0], 1 << 20, 1).is_none(),
            "a nine-byte length"
        );
        assert!(
            decode(&[0x11, 0x00, 0x05, 0x00], 1 << 20, 0).is_none(),
            "an empty run"
        );
        assert!(
            decode(&[0x11, 0x01, 0xFF, 0x00], 1 << 20, 1).is_none(),
            "before the volume"
        );
    }
}
