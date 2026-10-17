//! Scans a folder or volume and prints what the scanner found, how long it took and the process's peak memory.
//! Read-only. Used to measure the scanner on a real drive:
//!
//! ```text
//! cargo run --release -p vacancy-core --example scan -- C:\ [--threads N]
//! ```

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

use vacancy_core::{CancelToken, Env, ItemRef, ScanOptions, categorize, scan, volume_info};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = PathBuf::from(".");
    let mut options = ScanOptions::default();
    while let Some(arg) = args.next() {
        if arg == "--threads" {
            options.threads = args.next().and_then(|n| n.parse::<NonZeroUsize>().ok());
        } else {
            path = PathBuf::from(arg);
        }
    }
    let mut last = Duration::ZERO;
    let snapshot = scan(
        &path,
        &options,
        &mut |p| {
            if p.elapsed - last >= Duration::from_secs(5) {
                last = p.elapsed;
                eprintln!(
                    "{:>5.1}s  {:>10} files  {:>8} folders  {:>9.2} GB",
                    p.elapsed.as_secs_f64(),
                    p.files,
                    p.folders,
                    p.bytes as f64 / 1e9,
                );
            }
        },
        &CancelToken::new(),
    )
    .expect("scan");
    let s = snapshot.stats();
    println!("path            {}", path.display());
    println!("elapsed         {:.2} s", s.elapsed.as_secs_f64());
    println!("files           {}", s.files);
    println!("folders         {}", s.folders);
    println!(
        "bytes           {} ({:.2} GB)",
        s.bytes,
        s.bytes as f64 / 1e9
    );
    println!("unreadable      {}", s.unreadable);
    println!("links skipped   {}", s.links_skipped);
    println!("other volumes   {}", s.other_volumes_skipped);
    println!("hard links      {} extra names merged", s.hard_links_merged);
    println!("kept files      {}", snapshot.file_count());
    if let Ok(v) = volume_info(&path) {
        println!(
            "volume          {:.2} GB used of {:.2} GB; the scan counted {:.1}% of the used space",
            v.used_bytes as f64 / 1e9,
            v.total_bytes as f64 / 1e9,
            s.bytes as f64 / v.used_bytes as f64 * 100.0
        );
    }
    let mut unreadable: Vec<_> = snapshot.unreadable().map(|d| snapshot.path(d)).collect();
    unreadable.sort_by_key(|p| p.components().count());
    for p in unreadable.iter().take(25) {
        println!("  unreadable    {}", p.display());
    }
    let started = std::time::Instant::now();
    let cats = categorize(&snapshot, &Env::detect(&path));
    println!(
        "categorized in {:.0} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    for cat in cats.list().iter().filter(|c| c.bytes > 0) {
        println!(
            "{:<28} {:>9.2} GB  {:>6} items{}",
            cat.name,
            cat.bytes as f64 / 1e9,
            cat.items.len(),
            if cat.protected { "  (protected)" } else { "" }
        );
        for item in cat.items.iter().take(6) {
            let name = match item.at {
                ItemRef::Dir(d) => snapshot.path(d).display().to_string(),
                ItemRef::File(f) => snapshot.file_path(f).display().to_string(),
                ItemRef::FilesIn(d) => format!("files in {}", snapshot.path(d).display()),
            };
            println!("    {:>9.2} GB  {name}", item.bytes as f64 / 1e9);
        }
    }
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
            "peak memory     {:.1} MB working set, {:.1} MB private",
            c.PeakWorkingSetSize as f64 / 1e6,
            c.PeakPagefileUsage as f64 / 1e6
        );
    }
}

#[cfg(unix)]
fn peak_memory() {
    let mut u = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `u` is writable memory for one rusage.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, u.as_mut_ptr()) } == 0 {
        // SAFETY: getrusage returned 0, so it filled `u`.
        let u = unsafe { u.assume_init() };
        // ru_maxrss is bytes on macOS, kilobytes on Linux.
        let bytes = if cfg!(target_os = "macos") {
            u.ru_maxrss as f64
        } else {
            u.ru_maxrss as f64 * 1024.0
        };
        println!("peak memory     {:.1} MB resident", bytes / 1e6);
    }
}

#[cfg(not(any(windows, unix)))]
fn peak_memory() {}
