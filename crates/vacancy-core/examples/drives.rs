//! Lists the drives Vacancy offers to scan, the way the drive picker gets them, and how long listing took.
//! Read-only: it asks the OS about each volume and never lists or opens files.
//!
//! `cargo run -p vacancy-core --example drives`

use std::time::Instant;

use vacancy_core::list_volumes;

const GB: f64 = 1e9;

fn main() {
    let started = Instant::now();
    let drives = match list_volumes() {
        Ok(drives) => drives,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let took = started.elapsed();
    for d in &drives {
        println!(
            "{:<28} {:<22} {:<9} {:>9.1} GB total {:>9.1} GB used {:>9.1} GB free{}",
            d.name,
            d.mount_point,
            format!("{:?}", d.kind),
            d.total_bytes as f64 / GB,
            d.used_bytes as f64 / GB,
            d.free_bytes as f64 / GB,
            if d.system { "  (system)" } else { "" },
        );
    }
    println!(
        "{} drives in {:.1} ms",
        drives.len(),
        took.as_secs_f64() * 1000.0
    );
}
