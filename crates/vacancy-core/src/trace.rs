//! Opt-in timings for tuning the scanner and the steps after it. Set `VACANCY_SCAN_TRACE` to a file path and each
//! step of a scan appends a line `<unix ms>\t<step>\t<value>` to it, or set it to `-` to write them to stderr.
//! Unset (the default), each step costs one check and each folder nothing measurable. Nothing it writes reaches the
//! UI, and it never names a folder or a file: only step names, counts and durations.

use std::fmt::Display;
use std::io::Write;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Where trace lines go, read once from the environment.
fn target() -> Option<&'static str> {
    static TARGET: OnceLock<Option<String>> = OnceLock::new();
    TARGET
        .get_or_init(|| {
            std::env::var("VACANCY_SCAN_TRACE")
                .ok()
                .filter(|v| !v.is_empty())
        })
        .as_deref()
}

/// Whether tracing is on.
pub fn enabled() -> bool {
    target().is_some()
}

/// Writes one line: `step` and `value`, stamped with the wall clock in Unix milliseconds (so lines from the app's
/// shell and from the UI's side can be lined up).
pub fn note(step: &str, value: impl Display) {
    let Some(target) = target() else {
        return;
    };
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = format!("{ms}\t{step}\t{value}\n");
    if target == "-" {
        eprint!("{line}");
    } else if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(target)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Writes how long `step` took since `since`, in milliseconds.
pub fn took(step: &str, since: Instant) {
    if enabled() {
        note(step, format_args!("{:.1} ms", ms(since)));
    }
}

/// Milliseconds since `since`.
pub(crate) fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1e3
}

/// Writes how big `value` is as JSON (what the app's IPC sends to the UI) and how long encoding it took. Encodes it
/// only while tracing.
pub fn json_size(step: &str, value: &impl serde::Serialize) {
    if !enabled() {
        return;
    }
    let started = Instant::now();
    let bytes = serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0);
    note(
        step,
        format_args!("{bytes} bytes of JSON, encoded in {:.1} ms", ms(started)),
    );
}
