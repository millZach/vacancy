//! Reading a volume's file index: the boot sector, `$MFT`'s own record and runs (with its attribute list, when the
//! index is fragmented enough to need one), then every record, front to back, in big sequential reads.

use std::io;
use std::sync::mpsc;

use crate::boot::{self, Boot, BootError};
use crate::record::{
    self, ATTR_ATTRIBUTE_LIST, ATTR_DATA, Attr, Extent, FileRef, ListEntry, Sizes, Want,
};
use crate::runs::Run;
use crate::table::Table;
use crate::wire::Progress;

/// Something a volume can be read from: the volume itself, or an image of one. Reads may be at any offset and
/// length; a disk that needs aligned reads aligns them itself.
pub trait Disk: Send {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
    /// How many bytes it really holds: what the boot sector claims is checked against it.
    fn size(&mut self) -> io::Result<u64>;
}

impl Disk for std::fs::File {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        self.seek(SeekFrom::Start(offset))?;
        self.read_exact(buf)
    }
    fn size(&mut self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }
}

impl Disk for io::Cursor<Vec<u8>> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        self.seek(SeekFrom::Start(offset))?;
        self.read_exact(buf)
    }
    fn size(&mut self) -> io::Result<u64> {
        Ok(self.get_ref().len() as u64)
    }
}

/// Why the index couldn't be read.
#[derive(Debug)]
pub enum IndexError {
    /// The volume isn't NTFS (or BitLocker has it locked).
    NotNtfs,
    /// The volume couldn't be read.
    Io(io::Error),
    /// The index doesn't make sense.
    Unreadable(&'static str),
    /// The app's account can't list the drive's root, or folders couldn't be checked against it.
    Access,
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotNtfs => write!(f, "the volume isn't NTFS"),
            Self::Io(e) => write!(f, "the volume couldn't be read: {e}"),
            Self::Unreadable(why) => write!(f, "the file index can't be read: {why}"),
            Self::Access => write!(f, "folders couldn't be checked against the app's account"),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<io::Error> for IndexError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// An opened volume: its geometry and where `$MFT`'s records are.
#[derive(Debug)]
pub struct Volume<D> {
    disk: D,
    pub boot: Boot,
    mft: Vec<Run>,
    /// Records in the index.
    pub records: u32,
}

/// Records read at a time: 4 MiB with 1 KiB records (tests use small chunks, so their small volumes take several).
const CHUNK_RECORDS: usize = if cfg!(test) { 8 } else { 4096 };
/// `$Secure:$SDS` is a few MiB on a big volume; more than this is not a real one.
const LONGEST_SDS: u64 = 512 << 20;

impl<D: Disk> Volume<D> {
    /// Reads the boot sector and `$MFT`'s record.
    pub fn open(mut disk: D) -> Result<Self, IndexError> {
        let mut sector = vec![0u8; 512];
        disk.read_at(0, &mut sector)?;
        let boot = boot::parse(&sector).map_err(|e| match e {
            BootError::NotNtfs => IndexError::NotNtfs,
            BootError::Malformed => IndexError::Unreadable("the boot sector's geometry"),
        })?;
        let cluster = u64::from(boot.cluster_size);
        // The boot sector's size against the drive's own: a volume claiming more than its device holds is not one
        // (its last cluster may sit a sector short of the end, where NTFS keeps its boot sector's copy).
        let device = disk.size()?;
        if boot.total_clusters.saturating_mul(cluster) > device.saturating_add(cluster) {
            return Err(IndexError::Unreadable(
                "the boot sector claims more than the drive holds",
            ));
        }
        let want = Want {
            runs: true,
            total_clusters: boot.total_clusters,
            cluster_size: cluster,
        };
        let size = boot.record_size as usize;
        let mut buf = vec![0u8; size];
        let mft_at = boot
            .mft_lcn
            .checked_mul(cluster)
            .ok_or(IndexError::Unreadable("the boot sector's geometry"))?;
        disk.read_at(mft_at, &mut buf)?;
        let first = parse_mft_record(&mut buf, 0, want)?;
        let mut extents = first.extents;
        let sizes = first
            .sizes
            .ok_or(IndexError::Unreadable("$MFT has no size"))?;
        let mut runs = flatten(&extents);
        // A fragmented index lists the rest of its runs in extension records, found through the runs so far.
        let list = match (first.list, first.list_extent) {
            (Some(list), _) => list,
            (None, Some(extent)) => {
                let len = first
                    .list_size
                    .ok_or(IndexError::Unreadable("$MFT's attribute list has no size"))?;
                if len > 1 << 20 {
                    return Err(IndexError::Unreadable("$MFT's attribute list is too long"));
                }
                let mut bytes = vec![0u8; len as usize];
                read_stream(&mut disk, &extent.runs, cluster, 0, &mut bytes)?;
                record::list_entries(&bytes)
                    .ok_or(IndexError::Unreadable("$MFT's attribute list"))?
            }
            (None, None) => Vec::new(),
        };
        for entry in list
            .iter()
            .filter(|e| e.kind == ATTR_DATA && e.name.is_empty() && e.record.record != 0)
        {
            let at = entry
                .record
                .record
                .checked_mul(size as u64)
                .ok_or(IndexError::Unreadable("$MFT's attribute list"))?;
            read_stream(&mut disk, &runs, cluster, at, &mut buf)?;
            let more = parse_mft_record(&mut buf, entry.record.record, want)?;
            extents.extend(more.extents);
            runs = flatten(&extents);
        }
        let covered = runs
            .iter()
            .try_fold(0u64, |n, r| n.checked_add(r.clusters))
            .and_then(|n| n.checked_mul(cluster))
            .ok_or(IndexError::Unreadable("$MFT's runs"))?;
        let volume = boot.total_clusters.saturating_mul(cluster);
        if sizes.logical > covered || sizes.logical > volume {
            return Err(IndexError::Unreadable("$MFT's runs don't cover it"));
        }
        let records = u32::try_from(sizes.logical / size as u64)
            .map_err(|_| IndexError::Unreadable("more records than NTFS allows"))?;
        if records < 16 {
            return Err(IndexError::Unreadable("too few records"));
        }
        Ok(Self {
            disk,
            boot,
            mft: runs,
            records,
        })
    }

    /// Reads every record into a [`Table`], `progress` hearing how far it has got after each chunk (it may return
    /// false to stop: then the read ends with what it has). Reading and parsing overlap: one thread reads the next
    /// chunk while this one parses the last.
    pub fn read_table(
        &mut self,
        progress: &mut dyn FnMut(&Progress) -> bool,
    ) -> Result<Table, IndexError> {
        let size = self.boot.record_size as usize;
        let cluster = u64::from(self.boot.cluster_size);
        let total = self.records as usize;
        let mut table = Table::new(self.records, self.boot.total_clusters, cluster)
            .ok_or(IndexError::Unreadable("not enough memory for the index"))?;
        let (full_tx, full_rx) = mpsc::sync_channel::<io::Result<(usize, Vec<u8>)>>(2);
        let (empty_tx, empty_rx) = mpsc::channel::<Vec<u8>>();
        for _ in 0..3 {
            let _ = empty_tx.send(vec![0u8; CHUNK_RECORDS * size]);
        }
        let disk = &mut self.disk;
        let runs = &self.mft;
        let mut stopped = false;
        let result = std::thread::scope(|s| {
            s.spawn(move || {
                let mut first = 0usize;
                while first < total {
                    let Ok(mut buf) = empty_rx.recv() else { return };
                    let n = CHUNK_RECORDS.min(total - first);
                    buf.resize(n * size, 0);
                    let read = read_stream(disk, runs, cluster, (first * size) as u64, &mut buf);
                    let failed = read.is_err();
                    if full_tx.send(read.map(|()| (first, buf))).is_err() || failed {
                        return;
                    }
                    first += n;
                }
            });
            // Owned here, so that however this ends (an error included) they are dropped before the scope waits
            // for the reading thread, which then stops instead of waiting for a buffer that never comes.
            let full_rx = full_rx;
            let empty_tx = empty_tx;
            let mut done = 0usize;
            while done < total {
                let (first, mut buf) = match full_rx.recv() {
                    Ok(Ok(chunk)) => chunk,
                    Ok(Err(e)) => return Err(IndexError::Io(e)),
                    Err(_) => return Err(IndexError::Unreadable("the reader stopped")),
                };
                for (i, rec) in buf.chunks_exact_mut(size).enumerate() {
                    table.add((first + i) as u32, rec);
                }
                done = first + buf.len() / size;
                let _ = empty_tx.send(buf);
                let go_on = progress(&Progress {
                    records_done: done as u64,
                    records_total: total as u64,
                    files: table.files,
                    folders: table.folders,
                });
                if !go_on {
                    stopped = true;
                    break;
                }
            }
            Ok(())
        });
        result?;
        if stopped {
            return Err(IndexError::Unreadable("stopped"));
        }
        table.finish();
        if table.overflow {
            return Err(IndexError::Unreadable(
                "more names than the reader can hold",
            ));
        }
        Ok(table)
    }

    /// Reads the security descriptors too big for their records into the table (each at most 64 KiB, as Windows
    /// allows; a longer one stays unknown, and its folder locked).
    pub fn read_far_descriptors(&mut self, table: &mut Table) -> Result<(), IndexError> {
        let cluster = u64::from(self.boot.cluster_size);
        let far: Vec<(u32, Vec<Run>, u64)> = table
            .far_descriptors()
            .map(|(r, runs, len)| (r, runs.to_vec(), len))
            .collect();
        // Each at most 64 KiB, all together at most 64 MiB: more than that is no volume's.
        let mut budget: u64 = 64 << 20;
        for (record, runs, len) in far {
            if len == 0 || len > 64 * 1024 || len > budget {
                continue;
            }
            budget -= len;
            let mut sd = vec![0u8; len as usize];
            if read_stream(&mut self.disk, &runs, cluster, 0, &mut sd).is_ok() {
                table.set_descriptor(record, sd);
            }
        }
        Ok(())
    }

    /// `$Secure:$SDS`, read whole, if the table found it.
    pub fn read_sds(&mut self, table: &Table) -> Result<Vec<u8>, IndexError> {
        let size = table
            .sds_size
            .ok_or(IndexError::Unreadable("no $Secure:$SDS"))?;
        if size > LONGEST_SDS {
            return Err(IndexError::Unreadable("$Secure:$SDS is too long"));
        }
        let runs = flatten(&table.sds);
        let mut bytes = vec![0u8; size as usize];
        read_stream(
            &mut self.disk,
            &runs,
            u64::from(self.boot.cluster_size),
            0,
            &mut bytes,
        )?;
        Ok(bytes)
    }
}

struct MftRecord {
    extents: Vec<Extent>,
    sizes: Option<Sizes>,
    list: Option<Vec<ListEntry>>,
    list_extent: Option<Extent>,
    list_size: Option<u64>,
}

fn parse_mft_record(buf: &mut [u8], number: u64, want: Want) -> Result<MftRecord, IndexError> {
    record::apply_fixups(buf).map_err(|_| IndexError::Unreadable("$MFT's record"))?;
    let header = record::header(buf).map_err(|_| IndexError::Unreadable("$MFT's record"))?;
    let base = header.base.map_or(0, |b: FileRef| b.record);
    if !header.in_use() || (number != 0 && base != 0) || (number == 0 && header.base.is_some()) {
        return Err(IndexError::Unreadable("$MFT's record isn't in use"));
    }
    let mut out = MftRecord {
        extents: Vec::new(),
        sizes: None,
        list: None,
        list_extent: None,
        list_size: None,
    };
    record::attributes(buf, want, &mut |a| match a {
        Attr::Runs {
            kind: ATTR_DATA,
            name,
            extent,
            sizes,
        } if name.is_empty() => {
            if sizes.is_some() {
                out.sizes = sizes;
            }
            out.extents.push(extent);
        }
        Attr::Runs {
            kind: ATTR_ATTRIBUTE_LIST,
            extent,
            sizes,
            ..
        } => {
            out.list_size = sizes.map(|s| s.logical);
            out.list_extent = Some(extent);
        }
        Attr::List(Some(entries)) => out.list = Some(entries),
        _ => {}
    })
    .map_err(|_| IndexError::Unreadable("$MFT's record"))?;
    Ok(out)
}

/// The runs of a stream's extents, in VCN order. Extents that don't follow on from each other end the list there.
fn flatten(extents: &[Extent]) -> Vec<Run> {
    let mut sorted: Vec<&Extent> = extents.iter().collect();
    sorted.sort_by_key(|e| e.lowest_vcn);
    let mut runs = Vec::new();
    let mut vcn = 0u64;
    for e in sorted {
        if e.lowest_vcn != vcn {
            break;
        }
        for r in &e.runs {
            let Some(next) = vcn.checked_add(r.clusters) else {
                return runs;
            };
            vcn = next;
            runs.push(*r);
        }
    }
    runs
}

/// Reads `buf.len()` bytes of a stream from byte `offset`, through its runs. Holes read as zeros.
fn read_stream(
    disk: &mut dyn Disk,
    runs: &[Run],
    cluster: u64,
    offset: u64,
    buf: &mut [u8],
) -> io::Result<()> {
    let bad = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "a run past what a volume can hold",
        )
    };
    let mut want = offset;
    let mut filled = 0usize;
    let mut run_start = 0u64;
    for run in runs {
        let run_bytes = run.clusters.checked_mul(cluster).ok_or_else(bad)?;
        let run_end = run_start.checked_add(run_bytes).ok_or_else(bad)?;
        while filled < buf.len() && want < run_end && want >= run_start {
            let into = want - run_start;
            let n = usize::try_from(run_end - want)
                .unwrap_or(usize::MAX)
                .min(buf.len() - filled);
            let part = &mut buf[filled..filled + n];
            match run.lcn {
                Some(lcn) => {
                    let at = lcn
                        .checked_mul(cluster)
                        .and_then(|a| a.checked_add(into))
                        .ok_or_else(bad)?;
                    disk.read_at(at, part)?;
                }
                None => part.fill(0),
            }
            filled += n;
            want += n as u64;
        }
        if filled == buf.len() {
            return Ok(());
        }
        run_start = run_end;
    }
    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "past the end of the stream",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mem(Vec<u8>);
    impl Disk for Mem {
        fn size(&mut self) -> io::Result<u64> {
            Ok(self.0.len() as u64)
        }
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            let at = offset as usize;
            let src = self
                .0
                .get(at..at + buf.len())
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
            buf.copy_from_slice(src);
            Ok(())
        }
    }

    #[test]
    fn reads_a_stream_across_runs_and_holes() {
        let mut disk = Mem((0..64u8).collect());
        let runs = [
            Run {
                lcn: Some(4),
                clusters: 2,
            },
            Run {
                lcn: None,
                clusters: 1,
            },
            Run {
                lcn: Some(0),
                clusters: 1,
            },
        ];
        let mut buf = [0u8; 10];
        read_stream(&mut disk, &runs, 4, 6, &mut buf).unwrap();
        assert_eq!(buf, [22, 23, 0, 0, 0, 0, 0, 1, 2, 3]);
        let mut buf = [0u8; 10];
        read_stream(&mut disk, &runs, 4, 0, &mut buf).unwrap();
        assert_eq!(buf, [16, 17, 18, 19, 20, 21, 22, 23, 0, 0]);
        let mut big = [0u8; 20];
        assert!(read_stream(&mut disk, &runs, 4, 0, &mut big).is_err());
    }
}
