//! Compares fast scan with the normal scan on a real drive, read-only, and writes what it found. Windows asks for
//! admin for each fast scan: answer the prompt (Yes to measure it, No to see the fallback).
//!
//! ```text
//! cargo run --release -p vacancy-core --example fast_compare -- C:\ [--modes fast,normal] [--out report.md]
//! ```
//!
//! `--modes` is the scans to run, in order (`fast`, `normal`; default `fast,normal`). With both, the report compares
//! the last of each: totals, categories (normal against fast), the locked folders only fast scan counts, and how
//! many folders list exactly the same entries in both (the digests the executor checks before deleting). Every
//! scan's times go in it too: for a fast scan, how long the admin prompt waited, how long reading the index took,
//! and how long sorting it into the map took. Nothing is planned or deleted.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use vacancy_core::fast::agreement;
use vacancy_core::{
    CancelToken, Env, FastStage, Moment, ScanMethod, ScannedVolume, fast_scan_for_map,
    scan_for_map, volume_info,
};

struct Run {
    label: String,
    scanned: ScannedVolume,
    total: Duration,
    asking: Option<Duration>,
    reading: Option<Duration>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = PathBuf::from(r"C:\");
    let mut modes = vec!["fast".to_owned(), "normal".to_owned()];
    let mut out: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--modes" => {
                modes = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .map(str::to_owned)
                    .collect()
            }
            "--out" => out = args.next().map(PathBuf::from),
            _ => path = PathBuf::from(arg),
        }
    }
    let env = Env::detect(&path);
    let mut runs: Vec<Run> = Vec::new();
    for (i, mode) in modes.iter().enumerate() {
        let started = Instant::now();
        let mut asking_at: Option<Instant> = None;
        let mut reading_at: Option<Instant> = None;
        let mut sorting_at: Option<Instant> = None;
        let mut last = Instant::now();
        let mut on_update = |u: vacancy_core::ScanUpdate| {
            match u.stage {
                Some(FastStage::Asking) => {
                    asking_at.get_or_insert_with(Instant::now);
                }
                Some(FastStage::Reading) => {
                    reading_at.get_or_insert_with(Instant::now);
                }
                Some(FastStage::Sorting) => {
                    sorting_at.get_or_insert_with(Instant::now);
                }
                None => {}
            }
            if last.elapsed() >= Duration::from_secs(5) {
                last = Instant::now();
                eprintln!(
                    "{:>6.1}s {mode}: {} files, {} folders{}",
                    started.elapsed().as_secs_f64(),
                    u.files,
                    u.folders,
                    u.fallback
                        .map(|f| format!(" (fell back: {f:?})"))
                        .unwrap_or_default()
                );
            }
        };
        let moment = Moment::detect();
        let scanned = if mode == "fast" {
            eprintln!(
                "Fast scan of {}: Windows will ask for admin.",
                path.display()
            );
            fast_scan_for_map(
                &path,
                &env,
                &moment,
                &|_| volume_info(&path).ok(),
                None,
                &mut on_update,
                &CancelToken::new(),
            )
        } else {
            scan_for_map(&path, &env, &moment, &mut on_update, &CancelToken::new())
        }
        .expect("scan");
        let total = started.elapsed();
        // The admin prompt's wait: from asking until the reader sent anything (or the fallback started).
        let first_read = reading_at.or(sorting_at);
        runs.push(Run {
            label: format!("{} #{}", mode, i + 1),
            asking: asking_at.zip(first_read).map(|(a, r)| r - a),
            reading: first_read.map(|r| total.saturating_sub(r - started)),
            scanned,
            total,
        });
    }
    let report = report(&path, &runs);
    println!("{report}");
    if let Some(out) = out {
        std::fs::write(&out, &report).expect("write the report");
        eprintln!("wrote {}", out.display());
    }
}

fn gb(b: u64) -> String {
    format!("{:.2} GB", b as f64 / 1e9)
}

fn report(path: &std::path::Path, runs: &[Run]) -> String {
    let mut r = String::new();
    let _ = writeln!(
        r,
        "# Fast scan compared with the normal scan: {}\n",
        path.display()
    );
    let _ = writeln!(
        r,
        "| scan | method | fallback | total s | admin prompt s | index read and map s | files | folders | scanned | locked | records | unseen | over |"
    );
    let _ = writeln!(r, "|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for run in runs {
        let s = &run.scanned.overview.scan;
        let locked: u64 = s.locked.iter().map(|g| g.bytes).sum();
        let _ = writeln!(
            r,
            "| {} | {:?} | {} | {:.1} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            run.label,
            s.method,
            s.fallback.map(|f| format!("{f:?}")).unwrap_or_default(),
            run.total.as_secs_f64(),
            run.asking
                .map(|d| format!("{:.1}", d.as_secs_f64()))
                .unwrap_or_default(),
            run.reading
                .map(|d| format!("{:.1}", d.as_secs_f64()))
                .unwrap_or_default(),
            s.files,
            s.folders,
            gb(s.scanned_bytes),
            gb(locked),
            gb(s.records_bytes),
            gb(s.hidden_bytes),
            gb(s.over_count_bytes),
        );
    }
    let fast = runs
        .iter()
        .rev()
        .find(|r| r.scanned.overview.scan.method == ScanMethod::Fast);
    let normal = runs.iter().rev().find(|r| {
        r.scanned.overview.scan.method == ScanMethod::Normal && r.label.starts_with("normal")
    });
    if let Some(f) = fast {
        let _ = writeln!(r, "\n## Locked folders (fast scan, totals only)\n");
        for g in &f.scanned.overview.scan.locked {
            let _ = writeln!(
                r,
                "- {:?}: {} in {} files, {} folders",
                g.kind,
                gb(g.bytes),
                g.files,
                g.folders
            );
        }
    }
    let (Some(f), Some(n)) = (fast, normal) else {
        return r;
    };
    let _ = writeln!(r, "\n## Categories (normal, fast, fast minus normal)\n");
    let _ = writeln!(r, "| category | normal | fast | difference | % of normal |");
    let _ = writeln!(r, "|---|---|---|---|---|");
    let mut ids: Vec<_> = n.scanned.overview.categories.iter().map(|c| c.id).collect();
    for c in &f.scanned.overview.categories {
        if !ids.contains(&c.id) {
            ids.push(c.id);
        }
    }
    for id in ids {
        let bytes = |run: &Run| {
            run.scanned
                .overview
                .categories
                .iter()
                .find(|c| c.id == id)
                .map_or(0, |c| c.bytes)
        };
        let (a, b) = (bytes(n), bytes(f));
        let d = b as i128 - a as i128;
        let pct = if a > 0 {
            d as f64 / a as f64 * 100.0
        } else {
            0.0
        };
        let _ = writeln!(
            r,
            "| {id:?} | {} | {} | {:+.3} GB | {pct:+.2}% |",
            gb(a),
            gb(b),
            d as f64 / 1e9
        );
    }
    let a = agreement(&n.scanned.snapshot, &f.scanned.snapshot);
    let _ = writeln!(r, "\n## Folder by folder (matched by file id)\n");
    let _ = writeln!(
        r,
        "- in both: {}; only in the normal scan: {}; only in the fast scan: {}",
        a.folders_both, a.only_normal, a.only_fast
    );
    let pct = |x: u64| {
        if a.listed_both == 0 {
            0.0
        } else {
            x as f64 / a.listed_both as f64 * 100.0
        }
    };
    let _ = writeln!(
        r,
        "- listed by both: {}; the same entries (digests): {} ({:.3}%); the same own bytes and files: {} ({:.3}%)",
        a.listed_both,
        a.digests_equal,
        pct(a.digests_equal),
        a.own_equal,
        pct(a.own_equal)
    );
    let _ = writeln!(r, "\nFolders that differ (first {}):\n", a.examples.len());
    for e in &a.examples {
        let _ = writeln!(r, "- {e}");
    }
    r
}
