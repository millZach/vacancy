//! Starting the elevated reader and reading its stream. See ADR 0007.
//!
//! 1. The reader, `vacancy-index-reader.exe` beside the app, is opened so it can't be changed, renamed or deleted
//!    (no write or delete sharing; Windows also refuses to rename a folder a file is open in), and, in a build that
//!    pins it, its SHA-256 must be the one the build recorded.
//! 2. Windows starts it elevated ("runas"): its admin prompt is the user's to answer. No prompt answered yes, no
//!    read.
//! 3. The app connects to the pipe the reader made, as a client allowing identification only: the reader may check
//!    folders against the app's account, never act as it. The pipe's server must be the process Windows started.
//! 4. The app sends the fixed hello and reads; nothing else ever goes to the reader.

use std::ffi::OsStr;
use std::io::{self, Read};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY,
    GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_WRITE_DATA,
    OPEN_EXISTING, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SYNCHRONIZE,
    WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeServerProcessId, PeekNamedPipe, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{GetProcessId, TerminateProcess, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    ShellExecuteExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::scan::CancelToken;
use vacancy_index::wire::HELLO;

/// The reader's file name, beside the app's.
pub(crate) const READER: &str = "vacancy-index-reader.exe";

/// The reader's SHA-256, recorded by the app's build when it bundles the reader (see `src-tauri/build.rs`).
const PINNED: Option<&str> = option_env!("VACANCY_READER_SHA256");

/// Why the reader didn't send anything.
#[derive(Debug)]
pub(crate) enum Launch {
    /// The admin prompt was answered No, or closed.
    Declined,
    /// The reader couldn't be started or didn't connect.
    Failed(String),
}

struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the handle is owned here and closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

fn failed(why: impl Into<String>) -> Launch {
    Launch::Failed(why.into())
}

/// Starts the reader for drive `letter`, connects, and hands `read` the stream. `owner` is the window the admin
/// prompt belongs to.
pub(crate) fn read_index<R>(
    letter: char,
    owner: Option<isize>,
    cancel: &CancelToken,
    read: impl FnOnce(&mut dyn Read) -> R,
) -> Result<R, Launch> {
    let exe = std::env::current_exe().map_err(|e| failed(format!("the app's folder: {e}")))?;
    let reader = exe
        .parent()
        .map(|d| d.join(READER))
        .ok_or_else(|| failed("the app's folder"))?;
    read_index_with(
        &reader,
        cancel,
        |path, token| start_elevated(path, letter, token, owner),
        read,
    )
}

/// [`read_index`] with the reader at `reader`, started by `start` (given its path and the pipe's token).
fn read_index_with<R>(
    reader: &Path,
    cancel: &CancelToken,
    start: impl FnOnce(&Path, &str) -> Result<Owned, Launch>,
    read: impl FnOnce(&mut dyn Read) -> R,
) -> Result<R, Launch> {
    let held = hold_and_check(reader)?;
    let token = random_token().map_err(|e| failed(format!("no random numbers: {e}")))?;
    // However this ends (read, cancelled, never connected), the reader gets a short grace to end on its own, and
    // is ended if it hasn't: nothing started here may linger elevated.
    let process = Reaper(start(reader, &token)?);
    // The reader is running from the bytes checked above; it may be let go now.
    drop(held);
    let pid = {
        // SAFETY: the process handle is open.
        unsafe { GetProcessId(process.0.0) }
    };
    let pipe = connect(&token, &process.0, pid, cancel)?;
    // Buffered: the stream is millions of small frames, and each read of the pipe is two calls to Windows.
    let mut stream = std::io::BufReader::with_capacity(
        1 << 20,
        PipeStream {
            pipe: &pipe,
            process: &process.0,
            cancel,
        },
    );
    let out = read(&mut stream);
    drop(stream);
    drop(pipe);
    report_memory(&process.0);
    Ok(out)
}

/// How long a reader may take to end once the app is done with it.
const GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// The reader's process: when dropped, it waits [`GRACE`] for it to end, then ends it.
struct Reaper(Owned);

impl Drop for Reaper {
    fn drop(&mut self) {
        let ms = GRACE.as_millis() as u32;
        // SAFETY: the process handle is open.
        if unsafe { WaitForSingleObject(self.0.0, ms) } == WAIT_TIMEOUT {
            // SAFETY: as above; a handle without the right fails, and the reader's own limits still end it.
            if unsafe { TerminateProcess(self.0.0, 1) } == 0 {
                eprintln!(
                    "fast scan: the reader didn't end and couldn't be ended: {}",
                    io::Error::last_os_error()
                );
            } else {
                // Ending a process is asynchronous: wait for it to be gone.
                // SAFETY: the process handle is open.
                unsafe { WaitForSingleObject(self.0.0, 2_000) };
            }
        }
    }
}

/// Notes the reader's peak memory, for measuring.
fn report_memory(process: &Owned) {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    // SAFETY: an all-zero PROCESS_MEMORY_COUNTERS is valid.
    let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: `c` is writable and sized in `cb`; the process handle is open.
    if unsafe { GetProcessMemoryInfo(process.0, &mut c, c.cb) } != 0 {
        eprintln!(
            "fast scan: the reader's peak memory: {:.0} MB working set, {:.0} MB private",
            c.PeakWorkingSetSize as f64 / 1e6,
            c.PeakPagefileUsage as f64 / 1e6
        );
    }
}

/// Opens the reader so nobody can change, rename or delete it (or the folder it is in) while it is held, and
/// checks it is the reader this build bundled.
fn hold_and_check(path: &Path) -> Result<Owned, Launch> {
    let w = wide(path.as_os_str());
    // SAFETY: `w` is NUL-terminated.
    let h = unsafe {
        CreateFileW(
            w.as_ptr(),
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(failed(format!(
            "{READER} can't be opened: {}",
            io::Error::last_os_error()
        )));
    }
    let held = Owned(h);
    match PINNED {
        Some(expected) => {
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; 1 << 16];
            let mut total = 0u64;
            loop {
                let mut n = 0u32;
                // SAFETY: `buf` is writable for its length; the handle is open for reading.
                let ok = unsafe {
                    ReadFile(
                        held.0,
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                        &mut n,
                        ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    return Err(failed(format!("{READER} can't be read")));
                }
                if n == 0 {
                    break;
                }
                total += u64::from(n);
                if total > 64 << 20 {
                    return Err(failed(format!("{READER} isn't the one this app came with")));
                }
                hasher.update(&buf[..n as usize]);
            }
            let got: String = hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !got.eq_ignore_ascii_case(expected) {
                return Err(failed(format!("{READER} isn't the one this app came with")));
            }
        }
        // A development build runs the reader built beside it.
        None if cfg!(debug_assertions) => {}
        None => {
            return Err(failed(
                "this build doesn't record which reader it came with",
            ));
        }
    }
    Ok(held)
}

/// 128 random bits as 32 lowercase hex digits: the pipe's name can't be guessed.
fn random_token() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    // SAFETY: `bytes` is writable for its length; the system's preferred generator needs no algorithm handle.
    let status = unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(io::Error::other(format!("BCryptGenRandom: {status:#x}")));
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Asks Windows to start the reader with admin. Blocks while the admin prompt is up.
fn start_elevated(
    reader: &Path,
    letter: char,
    token: &str,
    owner: Option<isize>,
) -> Result<Owned, Launch> {
    let file = wide(reader.as_os_str());
    let verb = wide(OsStr::new("runas"));
    let params = wide(OsStr::new(&format!("--volume {letter}: --pipe {token}")));
    // It starts in System32, never in a folder the user can write to.
    let system = system_dir().ok_or_else(|| failed("Windows' System32 folder can't be found"))?;
    let dir = wide(system.as_os_str());
    // SAFETY: an all-zero SHELLEXECUTEINFOW is valid before its fields are set.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.hwnd = owner.map_or(ptr::null_mut(), |h| h as _);
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.lpDirectory = dir.as_ptr();
    info.nShow = SW_HIDE;
    // SAFETY: every string is NUL-terminated and outlives the call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // SAFETY: reads this thread's last error.
        return Err(match unsafe { GetLastError() } {
            ERROR_CANCELLED => Launch::Declined,
            code => failed(format!(
                "Windows couldn't start {READER}: {}",
                io::Error::from_raw_os_error(code as i32)
            )),
        });
    }
    if info.hProcess.is_null() {
        return Err(failed(format!("Windows started {READER} without a handle")));
    }
    Ok(Owned(info.hProcess))
}

/// Windows' System32 folder, as Windows gives it.
fn system_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buf = vec![0u16; 1024];
    // SAFETY: `buf` holds 1024 writable units.
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    (n > 0 && n < buf.len()).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buf[..n])))
}

/// Connects to the reader's pipe as soon as it is there, while the reader runs, for up to 30 seconds.
fn connect(token: &str, process: &Owned, pid: u32, cancel: &CancelToken) -> Result<Owned, Launch> {
    let name = wide(OsStr::new(&format!(r"\\.\pipe\vacancy-index-{token}")));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        // SAFETY: `name` is NUL-terminated. Identification only: the reader may check access as this account,
        // never act as it.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            let pipe = Owned(h);
            let mut server = 0u32;
            // SAFETY: the pipe is open; `server` is writable.
            let ok = unsafe { GetNamedPipeServerProcessId(pipe.0, &mut server) };
            if ok == 0 || server != pid {
                return Err(failed("the pipe isn't the reader's"));
            }
            let mut written = 0u32;
            // SAFETY: HELLO is readable for its length; the pipe is open for writing.
            let ok = unsafe {
                WriteFile(
                    pipe.0,
                    HELLO.as_ptr(),
                    HELLO.len() as u32,
                    &mut written,
                    ptr::null_mut(),
                )
            };
            if ok == 0 || written as usize != HELLO.len() {
                return Err(failed("the reader didn't take the hello"));
            }
            return Ok(pipe);
        }
        // SAFETY: reads this thread's last error.
        let code = unsafe { GetLastError() };
        if code == ERROR_PIPE_BUSY {
            // SAFETY: `name` is NUL-terminated.
            unsafe { WaitNamedPipeW(name.as_ptr(), 200) };
        } else if code != ERROR_FILE_NOT_FOUND {
            return Err(failed(format!(
                "the reader's pipe: {}",
                io::Error::from_raw_os_error(code as i32)
            )));
        }
        // SAFETY: the process handle is open.
        if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
            return Err(failed(format!("{READER} stopped before it was ready")));
        }
        if cancel.is_cancelled() || Instant::now() >= deadline {
            return Err(failed(format!("{READER} didn't get ready")));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// How long the reader may send nothing before the app gives up on it.
const SILENCE: Duration = Duration::from_secs(120);

/// The stream, read without blocking for long: what the pipe holds is read; when it holds nothing the read waits a
/// little and asks again, so a cancel lands within a few milliseconds and a reader that died ends the stream.
struct PipeStream<'a> {
    pipe: &'a Owned,
    process: &'a Owned,
    cancel: &'a CancelToken,
}

impl Read for PipeStream<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut exited_at: Option<Instant> = None;
        let waiting = Instant::now();
        loop {
            if self.cancel.is_cancelled() {
                return Err(io::Error::other("cancelled"));
            }
            // The reader sends progress while it reads the index and entries while it lists: two minutes of
            // silence is a reader stuck (a drive that stopped answering), and the app runs the normal scan.
            if waiting.elapsed() > SILENCE {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let mut available = 0u32;
            // SAFETY: the pipe is open; only the count is asked for.
            let ok = unsafe {
                PeekNamedPipe(
                    self.pipe.0,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &mut available,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                // SAFETY: reads this thread's last error.
                return match unsafe { GetLastError() } {
                    // The reader closed its end: the stream is over.
                    ERROR_BROKEN_PIPE => Ok(0),
                    code => Err(io::Error::from_raw_os_error(code as i32)),
                };
            }
            if available > 0 {
                let want = buf.len().min(available as usize) as u32;
                let mut n = 0u32;
                // SAFETY: `buf` is writable for `want` bytes, and the pipe holds at least that many.
                let ok = unsafe {
                    ReadFile(self.pipe.0, buf.as_mut_ptr(), want, &mut n, ptr::null_mut())
                };
                if ok == 0 {
                    return Err(io::Error::last_os_error());
                }
                return Ok(n as usize);
            }
            // SAFETY: the process handle is open.
            let running = unsafe { WaitForSingleObject(self.process.0, 0) } == WAIT_TIMEOUT;
            match (running, exited_at) {
                (true, _) => {}
                (false, None) => exited_at = Some(Instant::now()),
                // The reader has exited, and nothing more came.
                (false, Some(t)) if t.elapsed() > Duration::from_secs(2) => return Ok(0),
                (false, Some(_)) => {}
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::windows::io::AsRawHandle;
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    use super::*;
    use crate::fast::{Expect, FastError, snapshot_from_stream};
    use crate::scan::ScanOptions;
    use crate::snapshot::Snapshot;

    /// The reader the workspace's tests built (its own package's integration tests need it), beside the test
    /// binaries' folder.
    fn built_reader() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let reader = exe.parent()?.parent()?.join(READER);
        reader.exists().then_some(reader)
    }

    /// Starts the reader as an ordinary child process (no admin prompt): its parent is this test.
    fn start_plain(letter: char) -> impl FnOnce(&Path, &str) -> Result<Owned, Launch> {
        move |path, token| {
            let child = std::process::Command::new(path)
                .args(["--volume", &format!("{letter}:"), "--pipe", token])
                .spawn()
                .map_err(|e| failed(e.to_string()))?;
            let mut dup: HANDLE = ptr::null_mut();
            // SAFETY: duplicates the child's process handle into this process; the child outlives the call.
            let ok = unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    child.as_raw_handle(),
                    GetCurrentProcess(),
                    &mut dup,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            assert!(ok != 0, "duplicate the child's handle");
            Ok(Owned(dup))
        }
    }

    /// The pipe end to end, without an admin prompt: the reader starts, makes its pipe, accepts this process (its
    /// parent) after the hello, and sends a stream the app reads. Not elevated, it can't open the volume and says
    /// so (the app falls back); elevated (CI's runners run as admin), it reads the whole volume's index, and the
    /// fast snapshot of a folder made here must list exactly what the normal scan of it lists.
    #[test]
    fn the_reader_answers_over_the_pipe() {
        let Some(reader) = built_reader() else {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI builds the reader with the workspace's tests"
            );
            eprintln!("skipped: the reader isn't built (run cargo test --workspace)");
            return;
        };
        let fx = tempfile::tempdir().unwrap();
        let root = fx.path().join("fast");
        crate::fixture::build(&root).expect("a fixture profile");
        let letter = root
            .to_str()
            .and_then(|s| s.chars().next())
            .filter(char::is_ascii_alphabetic)
            .expect("a drive letter");
        if !crate::volume::file_system_of(&root).eq_ignore_ascii_case("NTFS") {
            eprintln!("skipped: the temp folder isn't on NTFS");
            return;
        }
        let id = crate::fsid::identity(&root).expect("the folder's identity");
        let capacity = crate::volume_info(&root).unwrap().total_bytes;
        let expect = Expect {
            root: root.clone(),
            volume_serial: id.volume(),
            subtree: Some(id.scan_key() as u64),
            capacity,
        };
        let cancel = CancelToken::new();
        let result = read_index_with(&reader, &cancel, start_plain(letter), |stream| {
            snapshot_from_stream(
                stream,
                &expect,
                &ScanOptions::default(),
                &mut |_| {},
                &mut |_| {},
                &cancel,
            )
        })
        .expect("the reader starts and connects");
        match result {
            Err(FastError::Unreadable(why)) => {
                // Not elevated: the volume can't be opened, and the reader said so over the pipe. CI's Windows
                // runners run as admin, so there the whole read must happen.
                assert!(why.contains("couldn't be opened"), "{why}");
                assert!(
                    std::env::var_os("CI").is_none(),
                    "CI runs elevated: the reader should have read the volume ({why})"
                );
            }
            Err(e) => panic!("{e}"),
            Ok(fast) => {
                compare_with_normal(&root, &fast);
                plan_and_run_from_fast(&root, fast);
            }
        }
    }

    /// From the fast scan's snapshot, the rules propose what they propose from the normal scan's, the plan is the
    /// same, and it runs: the executor's checks by handle pass on the identities the index gave.
    fn plan_and_run_from_fast(root: &Path, fast: Snapshot) {
        use crate::rules::Moment;
        let env = crate::fixture::env(root);
        let moment = Moment {
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            running_apps: Some(Vec::new()),
            installed_apps: Vec::new(),
        };
        let figures_root = root.to_path_buf();
        let figures =
            move |snap: &Snapshot| Some(crate::fixture::volume(&figures_root, 1 << 30, snap));
        let cancel = CancelToken::new();
        let normal =
            crate::scan_for_map_with(root, &env, &moment, &figures, &mut |_| {}, &cancel).unwrap();
        let fast = crate::map::finish_map(
            fast,
            &env,
            &moment,
            root,
            &figures,
            crate::map::HowScanned {
                method: crate::map::ScanMethod::Fast,
                ..Default::default()
            },
            &cancel,
        )
        .unwrap();
        // The same cleanups, in the same places. A file with several names is counted where a scan meets it first:
        // the normal scan's threads race for it, the fast scan meets the shallower name first (the pnpm store
        // before the project linking into it), so a proposal's bytes may differ by such files, and only by them.
        let said = |s: &crate::ScannedVolume| {
            let mut v: Vec<(String, u64)> = s
                .proposals
                .list()
                .iter()
                .map(|p| (p.id.clone(), p.place_count))
                .collect();
            v.sort();
            v
        };
        assert_eq!(said(&normal), said(&fast), "the same proposals");
        let linked = fast.snapshot.stats().hard_links_merged;
        for p in fast.proposals.list() {
            let n = normal
                .proposals
                .list()
                .iter()
                .find(|q| q.id == p.id)
                .unwrap();
            if (p.bytes, p.item_count) != (n.bytes, n.item_count) {
                assert!(
                    linked > 0,
                    "{}: only hard links can be counted elsewhere",
                    p.id
                );
                assert!(p.item_count.abs_diff(n.item_count) <= linked, "{}", p.id);
            }
        }
        let ids: Vec<String> = fast
            .proposals
            .list()
            .iter()
            .filter(|p| p.default_on)
            .map(|p| p.id.clone())
            .collect();
        assert!(!ids.is_empty(), "the fixture has cleanups on by default");
        let normal_plan = crate::plan::plan(&normal, &env, &ids).unwrap().summary();
        let plan = crate::plan::plan(&fast, &env, &ids).unwrap();
        let summary = plan.summary();
        assert_eq!(
            (summary.places, summary.refused.len()),
            (normal_plan.places, normal_plan.refused.len()),
            "the same places planned, the same refused"
        );
        let documents = crate::fixture::inventory(&env.known.documents).unwrap();
        let report = crate::exec::execute(&plan, &env, &|| None, &mut |_| {}, &cancel);
        assert_eq!(
            report.freed_bytes, summary.bytes,
            "everything planned was freed: {report:#?}"
        );
        assert_eq!(
            crate::fixture::inventory(&env.known.documents).unwrap(),
            documents,
            "Documents untouched"
        );
    }

    /// A started reader that never answers (here a stand-in that just waits) is ended once the app gives up on it,
    /// a cancel included: nothing started for a fast scan outlives it by more than the grace.
    #[test]
    fn a_reader_that_never_answers_is_ended() {
        let Some(reader) = built_reader() else {
            assert!(std::env::var_os("CI").is_none(), "CI builds the reader");
            return;
        };
        let pid = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = pid.clone();
        let cancel = CancelToken::new();
        let stop = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            stop.cancel();
        });
        let started = std::time::Instant::now();
        let r = read_index_with(
            &reader,
            &cancel,
            move |_, _| {
                let child = std::process::Command::new("ping")
                    .args(["-n", "120", "127.0.0.1"])
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| failed(e.to_string()))?;
                seen.store(child.id(), std::sync::atomic::Ordering::SeqCst);
                let mut dup: HANDLE = ptr::null_mut();
                // SAFETY: duplicates the child's process handle into this process.
                let ok = unsafe {
                    DuplicateHandle(
                        GetCurrentProcess(),
                        child.as_raw_handle(),
                        GetCurrentProcess(),
                        &mut dup,
                        0,
                        0,
                        DUPLICATE_SAME_ACCESS,
                    )
                };
                assert!(ok != 0);
                Ok(Owned(dup))
            },
            |_| (),
        );
        assert!(matches!(r, Err(Launch::Failed(_))), "{r:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        let pid = pid.load(std::sync::atomic::Ordering::SeqCst);
        // SAFETY: opens the stand-in by id to see whether it still runs.
        let h = unsafe { windows_sys::Win32::System::Threading::OpenProcess(SYNCHRONIZE, 0, pid) };
        if !h.is_null() {
            let h = Owned(h);
            // SAFETY: the handle is open.
            let state = unsafe { WaitForSingleObject(h.0, 0) };
            assert_eq!(state, WAIT_OBJECT_0, "it was ended");
        }
    }

    /// A pipe someone else made first under the reader's name: the reader can't make its own (it exits), and the
    /// app refuses to talk to a server that isn't the reader. Nothing is read.
    #[test]
    fn a_squatted_pipe_is_refused() {
        let Some(reader) = built_reader() else {
            assert!(std::env::var_os("CI").is_none(), "CI builds the reader");
            return;
        };
        let fx = tempfile::tempdir().unwrap();
        let letter = fx.path().to_str().and_then(|s| s.chars().next()).unwrap();
        let squatted = std::rc::Rc::new(std::cell::RefCell::new(None));
        let keep = squatted.clone();
        let cancel = CancelToken::new();
        let plain = start_plain(letter);
        let r = read_index_with(
            &reader,
            &cancel,
            move |path, token| {
                use windows_sys::Win32::System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE};
                let name = wide(OsStr::new(&format!(r"\\.\pipe\vacancy-index-{token}")));
                // SAFETY: `name` is NUL-terminated; a plain duplex pipe with the default security.
                let h = unsafe {
                    CreateNamedPipeW(
                        name.as_ptr(),
                        3,
                        PIPE_TYPE_BYTE,
                        1,
                        4096,
                        4096,
                        0,
                        ptr::null(),
                    )
                };
                assert!(h != INVALID_HANDLE_VALUE, "the squatter's pipe");
                *keep.borrow_mut() = Some(Owned(h));
                plain(path, token)
            },
            |_| panic!("nothing is read from a squatted pipe"),
        );
        assert!(matches!(r, Err(Launch::Failed(_))), "{r:?}");
        drop(squatted);
    }

    fn compare_with_normal(root: &Path, fast: &Snapshot) {
        let normal = crate::scan(
            root,
            &ScanOptions::default(),
            &mut |_| {},
            &CancelToken::new(),
        )
        .expect("a normal scan");
        let a = crate::fast::agreement(&normal, fast);
        eprintln!("{a:?}");
        assert_eq!((a.only_normal, a.only_fast), (0, 0), "{a:?}");
        // Every folder lists the same entries. (Which folder a hard-linked file's bytes count in depends on which
        // name a scan meets first, so own totals may differ where the fixture has hard links; the totals can't.)
        assert_eq!(a.digests_equal, a.listed_both, "{a:?}");
        let (n, f) = (normal.stats(), fast.stats());
        assert_eq!((n.files, n.folders, n.bytes), (f.files, f.folders, f.bytes));
        assert_eq!(n.hard_links_merged, f.hard_links_merged);
        assert_eq!(n.links_skipped, f.links_skipped);
    }
}
