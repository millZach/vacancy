//! Measures fast scan on an NTFS image file, without admin: the reader's work (the index read, parsed and sent as
//! a stream, every folder open to the account) and the app's (the stream into a snapshot, then categories), with
//! times and peak memory. Read-only.
//!
//! ```text
//! cargo run --release -p vacancy-core --example fast_image -- D:\tmp\vacancy-ntfs\big.img
//! ```

use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

use vacancy_core::fast::{Expect, snapshot_from_stream};
use vacancy_core::{CancelToken, Env, ScanOptions, categorize};
use vacancy_index::{Access, Security};

struct Everyone;

impl Access for Everyone {
    fn can_list(&mut self, security: Security<'_>) -> bool {
        matches!(security, Security::Descriptor(_))
    }
}

fn main() {
    let path = PathBuf::from(std::env::args().nth(1).expect("an NTFS image"));
    let mut boot = [0u8; 512];
    std::fs::File::open(&path)
        .and_then(|mut f| f.read_exact(&mut boot))
        .expect("read the boot sector");
    let geometry = vacancy_index::boot::parse(&boot).expect("an NTFS boot sector");
    let capacity = std::fs::metadata(&path).expect("the image").len();

    let started = Instant::now();
    let disk = std::fs::File::open(&path).expect("open the image");
    let mut enc = vacancy_index::wire::Encoder::new(Vec::new());
    let summary = vacancy_index::serve(disk, &mut Everyone, &mut enc, &|| false)
        .expect("the index reads")
        .expect("not stopped");
    let stream = enc.into_inner();
    let read = started.elapsed();
    println!(
        "reader: {:.2} s, {} folders, {} entries, {} locked, {:.1} MB stream, records {:.2} GB",
        read.as_secs_f64(),
        summary.dirs,
        summary.entries,
        summary.locked,
        stream.len() as f64 / 1e6,
        summary.records_bytes as f64 / 1e9
    );

    let started = Instant::now();
    let snapshot = snapshot_from_stream(
        &stream[..],
        &Expect {
            root: PathBuf::from(r"X:\"),
            volume_serial: geometry.serial,
            subtree: None,
            capacity,
        },
        &ScanOptions::default(),
        &mut |_| {},
        &mut |_| {},
        &CancelToken::new(),
    )
    .expect("a snapshot");
    let s = snapshot.stats();
    println!(
        "app: snapshot in {:.2} s: {} files, {} folders, {:.2} GB, {} hard links merged, {} kept files",
        started.elapsed().as_secs_f64(),
        s.files,
        s.folders,
        s.bytes as f64 / 1e9,
        s.hard_links_merged,
        snapshot.file_count()
    );
    let started = Instant::now();
    let cats = categorize(&snapshot, &Env::windows(&PathBuf::from(r"X:\"), "nobody"));
    println!(
        "app: categorized in {:.0} ms ({} categories)",
        started.elapsed().as_secs_f64() * 1000.0,
        cats.list().len()
    );
    peak_memory();
}

#[cfg(windows)]
fn peak_memory() {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: an all-zero PROCESS_MEMORY_COUNTERS is valid.
    let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: `c` is writable and its size is in `cb`; the pseudo-handle needs no closing.
    if unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } != 0 {
        println!(
            "peak memory: {:.1} MB working set, {:.1} MB private (the reader and the app together)",
            c.PeakWorkingSetSize as f64 / 1e6,
            c.PeakPagefileUsage as f64 / 1e6
        );
    }
}

#[cfg(not(windows))]
fn peak_memory() {}
