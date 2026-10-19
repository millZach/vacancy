//! Vacancy's fast scan reads an NTFS volume's file index (the Master File Table) instead of opening every folder.
//! Reading the volume needs admin, so it happens in a small elevated reader, `vacancy-index-reader`; this crate is
//! that reader's parts, and the stream format the app reads back:
//!
//! - [`mft`] reads the boot sector and the index, and [`record`] each record, with its fixups, its attributes, the
//!   names in every namespace but DOS 8.3, and its streams' sizes. Never a file's contents: of `$DATA` only the sizes
//!   in its header are read.
//! - [`table`] keeps what fast scan needs of each record, merging extension records into their base record.
//! - [`emit`] decides which folders the app's account can open and sends the rest as totals only, over [`wire`].
//!
//! Everything but the reader's Windows calls is plain Rust over bytes, tested on every platform.

pub mod boot;
pub mod emit;
pub mod kind;
mod le;
pub mod mft;
pub mod record;
pub mod runs;
pub mod sd;
pub mod sds;
pub mod table;
pub mod wire;

#[cfg(test)]
mod pipeline_tests;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

#[cfg(windows)]
pub mod windows;

use std::io::Write;

pub use emit::{Access, Security, Summary};
pub use mft::{Disk, IndexError, Volume};

/// Reads the volume on `disk` and sends it to `out`: the header, progress while the index is read, then the
/// listing (see [`emit`]). `stop` is asked all along; when it says so, nothing more is sent. Errors are sent as an
/// `Error` frame before they are returned.
pub fn serve<D: Disk, W: Write>(
    disk: D,
    access: &mut dyn Access,
    out: &mut wire::Encoder<W>,
    stop: &dyn Fn() -> bool,
) -> Result<Option<Summary>, IndexError> {
    let result = serve_inner(disk, access, out, stop);
    if let Err(e) = &result {
        let code = match e {
            IndexError::NotNtfs => wire::ErrorCode::NotNtfs,
            IndexError::Io(_) | IndexError::Unreadable(_) => wire::ErrorCode::Unreadable,
            IndexError::Access => wire::ErrorCode::Access,
        };
        let _ = out.error(code);
    }
    result
}

fn serve_inner<D: Disk, W: Write>(
    disk: D,
    access: &mut dyn Access,
    out: &mut wire::Encoder<W>,
    stop: &dyn Fn() -> bool,
) -> Result<Option<Summary>, IndexError> {
    let mut volume = Volume::open(disk)?;
    out.header(&wire::Header {
        volume_serial: volume.boot.serial,
        cluster_size: volume.boot.cluster_size,
        record_size: volume.boot.record_size,
        records: u64::from(volume.records),
        total_clusters: volume.boot.total_clusters,
    })?;
    let mut last = std::time::Instant::now();
    let mut sent = Ok(());
    let table = volume.read_table(&mut |p| {
        if last.elapsed() >= std::time::Duration::from_millis(100) {
            last = std::time::Instant::now();
            if let Err(e) = out.progress(p) {
                sent = Err(e);
                return false;
            }
        }
        !stop()
    });
    sent?;
    if stop() {
        return Ok(None);
    }
    let mut table = table?;
    volume.read_far_descriptors(&mut table)?;
    // Volumes older than NTFS 3.0 have no $Secure: their folders carry their own descriptors.
    let sds = match table.sds_size {
        None => Vec::new(),
        Some(_) => volume.read_sds(&table)?,
    };
    let index = sds::index(&sds);
    emit::emit(&mut table, &sds, &index, access, out, stop).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            IndexError::Access
        } else {
            IndexError::Io(e)
        }
    })
}
