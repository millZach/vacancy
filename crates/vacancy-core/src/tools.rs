//! Closed, bounded tool queries. A discovery result is evidence, never authority to execute a cleanup.
//!
//! The native adapter deliberately exposes only read-only operations until a tool's approved scope can be
//! frozen. Fixture callers inject [`FakeRunner`]; it has no native adapter and cannot fall back to the host.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{CancelToken, Env};

/// A verified local socket spelling, never a TCP/SSH endpoint or a command argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEndpoint(String);
impl LocalEndpoint {
    pub fn parse(value: &str) -> Option<Self> {
        if value.len() <= 4096
            && !value.chars().any(char::is_control)
            && (value.starts_with("unix:///") || value.starts_with("npipe:////./pipe/"))
        {
            Some(Self(value.into()))
        } else {
            None
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockerRead {
    Info,
    Images,
    Containers,
    DiskUsage,
}

/// The complete query vocabulary. No caller supplies executable paths, shell text or arbitrary arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    DockerContext,
    Docker {
        endpoint: LocalEndpoint,
        read: DockerRead,
    },
    SimulatorInventory,
    SimulatorDeleteHelp,
    SimulatorRuntimeHelp,
    BrewPreview,
}

impl Query {
    fn command(&self) -> (&'static str, Vec<OsString>) {
        let (program, args): (&str, &[&str]) = match self {
            Self::DockerContext => ("docker", &["context", "inspect"]),
            Self::Docker { endpoint, read } => {
                let tail: &[&str] = match read {
                    DockerRead::Info => &["info", "--format", "{{json .}}"],
                    DockerRead::Images => &["image", "ls", "--no-trunc", "--format", "{{json .}}"],
                    DockerRead::Containers => &[
                        "container",
                        "ls",
                        "--all",
                        "--no-trunc",
                        "--format",
                        "{{json .}}",
                    ],
                    DockerRead::DiskUsage => {
                        &["system", "df", "--verbose", "--format", "{{json .}}"]
                    }
                };
                let mut args = vec![OsString::from("--host"), OsString::from(endpoint.as_str())];
                args.extend(tail.iter().map(OsString::from));
                return ("docker", args);
            }
            Self::SimulatorInventory => ("xcrun", &["simctl", "list", "--json"]),
            Self::SimulatorDeleteHelp => ("xcrun", &["simctl", "help", "delete"]),
            Self::SimulatorRuntimeHelp => ("xcrun", &["simctl", "help", "runtime"]),
            Self::BrewPreview => ("brew", &["cleanup", "--dry-run"]),
        };
        (program, args.iter().map(OsString::from).collect())
    }
}

/// Terminal states are separate from text output. A successful exit alone proves no reclaimed bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Success,
    Missing,
    Refused(String),
    Failed(Option<i32>),
    TimedOut,
    Cancelled,
}

/// Bounded raw output, retained for diagnostics and schema parsing. Parsers must reject truncated data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub status: Status,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
}

impl Output {
    fn stopped(status: Status) -> Self {
        Self {
            status,
            stdout: String::new(),
            stderr: String::new(),
            truncated: false,
        }
    }

    pub fn complete(&self) -> bool {
        self.status == Status::Success && !self.truncated
    }
}

/// Time and memory budgets apply even when the tool never exits or floods either pipe.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub timeout: Duration,
    pub output_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(20),
            output_bytes: 256 * 1024,
        }
    }
}

/// Discovery depends on this interface; fixture mode must receive a fake explicitly.
pub trait Runner {
    fn query(&mut self, query: Query, cancel: &CancelToken) -> Output;
}

/// Recorded responses only. Unscripted calls fail closed, even on a computer with the real tool installed.
#[derive(Debug, Default)]
pub struct FakeRunner {
    replies: VecDeque<(Query, Output)>,
    pub calls: Vec<Query>,
}

impl FakeRunner {
    pub fn new(replies: impl IntoIterator<Item = (Query, Output)>) -> Self {
        Self {
            replies: replies.into_iter().collect(),
            calls: Vec::new(),
        }
    }
}

impl Runner for FakeRunner {
    fn query(&mut self, query: Query, cancel: &CancelToken) -> Output {
        self.calls.push(query.clone());
        if cancel.is_cancelled() {
            return Output::stopped(Status::Cancelled);
        }
        match self.replies.pop_front() {
            Some((expected, output)) if expected == query => output,
            _ => Output::stopped(Status::Refused("No fixture response for this query".into())),
        }
    }
}

/// Native read-only adapter. Search paths are captured once, never through a shell or the working directory.
#[derive(Debug)]
pub struct NativeQueries {
    search: Vec<PathBuf>,
    limits: Limits,
}

impl NativeQueries {
    pub fn for_real(env: &Env, limits: Limits) -> Result<Self, String> {
        if env.confined_to.is_some() || std::env::var_os(crate::fixture::ROOT_VAR).is_some() {
            return Err("Fixture mode requires an explicitly injected fixture runner".into());
        }
        let search = std::env::var_os("PATH")
            .map(|p| {
                std::env::split_paths(&p)
                    .filter(|p| p.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self { search, limits })
    }
}

impl Runner for NativeQueries {
    fn query(&mut self, query: Query, cancel: &CancelToken) -> Output {
        if cancel.is_cancelled() {
            return Output::stopped(Status::Cancelled);
        }
        let (name, args) = query.command();
        let Some(program) = locate(&self.search, name) else {
            return Output::stopped(Status::Missing);
        };
        // No shell, inherited stdin, credential prompt, or current-directory lookup.
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .env("HOMEBREW_NO_ANALYTICS", "1");
        for key in [
            "DOCKER_HOST",
            "DOCKER_CONTEXT",
            "DOCKER_TLS_VERIFY",
            "DOCKER_CERT_PATH",
            "BUILDX_BUILDER",
        ] {
            command.env_remove(key);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        run(command, self.limits, cancel)
    }
}

fn locate(search: &[PathBuf], name: &str) -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        OsString::from(format!("{name}.exe"))
    } else {
        OsString::from(name)
    };
    search
        .iter()
        .map(|p| p.join(&filename))
        .find(|p| p.is_file())
}

fn run(mut command: Command, limits: Limits, cancel: &CancelToken) -> Output {
    let tree = match process_tree::prepare(&mut command) {
        Ok(tree) => tree,
        Err(e) => {
            return Output::stopped(Status::Refused(format!("Cannot contain tool process: {e}")));
        }
    };
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return Output::stopped(Status::Refused(e.to_string())),
    };
    let _tree = match tree.attach(&child) {
        Ok(tree) => tree,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Output::stopped(Status::Refused(format!("Cannot contain tool process: {e}")));
        }
    };
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    if let Err(e) = pipe::prepare(&stdout).and_then(|()| pipe::prepare(&stderr)) {
        let _ = child.kill();
        let _ = child.wait();
        return Output::stopped(Status::Refused(format!("Cannot bound tool output: {e}")));
    }
    let start = Instant::now();
    let cap = limits.output_bytes.min(1024 * 1024);
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut truncated = false;
    let status = loop {
        // Each pass drains at most 16 KiB per pipe so a continuous writer cannot starve cancel/deadline checks.
        if let Err(e) = drain(&mut stdout, &mut out, cap, &mut truncated)
            .and_then(|()| drain(&mut stderr, &mut err, cap, &mut truncated))
        {
            let _ = child.kill();
            let _ = child.wait();
            return Output::stopped(Status::Refused(format!("Cannot read tool output: {e}")));
        }
        let stop = if cancel.is_cancelled() {
            Some(Status::Cancelled)
        } else if start.elapsed() >= limits.timeout {
            Some(Status::TimedOut)
        } else {
            None
        };
        if let Some(status) = stop {
            let _ = child.kill();
            let _ = child.wait();
            break status;
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                // Pipes may be inherited by a descendant. Never wait for EOF after the owned child exits.
                // Remaining output is bounded; mark it incomplete rather than inventing a full transcript.
                for _ in 0..64 {
                    let before = (out.len(), err.len());
                    let _ = drain(&mut stdout, &mut out, cap, &mut truncated);
                    let _ = drain(&mut stderr, &mut err, cap, &mut truncated);
                    if before == (out.len(), err.len()) {
                        break;
                    }
                }
                break if exit.success() {
                    Status::Success
                } else {
                    Status::Failed(exit.code())
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Status::Refused(e.to_string());
            }
        }
    };
    Output {
        status,
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
        truncated,
    }
}

fn drain<R: Read + pipe::Pipe>(
    pipe: &mut R,
    data: &mut Vec<u8>,
    cap: usize,
    truncated: &mut bool,
) -> io::Result<()> {
    let mut buffer = [0; 16 * 1024];
    let available = pipe::available(pipe)?.min(buffer.len());
    if available == 0 {
        return Ok(());
    }
    match pipe.read(&mut buffer[..available]) {
        Ok(n) => {
            let keep = n.min(cap.saturating_sub(data.len()));
            data.extend_from_slice(&buffer[..keep]);
            *truncated |= keep < n;
            Ok(())
        }
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::BrokenPipe
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
mod pipe {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    pub trait Pipe: AsRawHandle {}
    impl<T: AsRawHandle> Pipe for T {}
    pub fn prepare<T: Pipe>(_: &T) -> io::Result<()> {
        Ok(())
    }
    pub fn available<T: Pipe>(pipe: &T) -> io::Result<usize> {
        use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA};
        use windows_sys::Win32::System::Pipes::PeekNamedPipe;
        let mut bytes = 0;
        // SAFETY: the pipe owns a valid read handle; only the valid `bytes` output pointer is used.
        if unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut bytes,
                std::ptr::null_mut(),
            )
        } != 0
        {
            Ok(bytes as usize)
        } else {
            let e = io::Error::last_os_error();
            if matches!(
                e.raw_os_error().map(|n| n as u32),
                Some(ERROR_BROKEN_PIPE | ERROR_NO_DATA)
            ) {
                Ok(0)
            } else {
                Err(e)
            }
        }
    }
}

#[cfg(unix)]
mod pipe {
    use std::io;
    use std::os::fd::AsRawFd;
    pub trait Pipe: AsRawFd {}
    impl<T: AsRawFd> Pipe for T {}
    pub fn prepare<T: Pipe>(pipe: &T) -> io::Result<()> {
        // SAFETY: fcntl receives a live pipe descriptor and integer flags, with no pointers.
        let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
        // SAFETY: same live descriptor, preserving its existing flags.
        if flags < 0
            || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    pub fn available<T: Pipe>(_: &T) -> io::Result<usize> {
        Ok(16 * 1024)
    }
}

pub mod discovery;

#[cfg(unix)]
mod process_tree {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};
    #[derive(Debug)]
    pub struct Prepared;
    pub struct Tree(u32);
    pub fn prepare(command: &mut Command) -> io::Result<Prepared> {
        command.process_group(0);
        Ok(Prepared)
    }
    impl Prepared {
        pub fn attach(self, child: &Child) -> io::Result<Tree> {
            Ok(Tree(child.id()))
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            // SAFETY: the spawned child was made leader of its own process group. Negative pid targets that
            // group only, never the app's group. Descendants retaining pipes cannot outlive this invocation.
            unsafe {
                libc::kill(-(self.0 as libc::pid_t), libc::SIGKILL);
            }
        }
    }
}

#[cfg(windows)]
mod process_tree {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    pub struct Tree(HANDLE);
    impl Drop for Tree {
        fn drop(&mut self) {
            /* SAFETY: this wrapper exclusively owns the job handle. */
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub fn prepare(command: &mut Command) -> io::Result<Tree> {
        // SAFETY: null security/name requests an unnamed job with default security, owned by this process.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        let tree = Tree(job);
        // SAFETY: zero is valid initialization of this plain Windows information struct.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: info points to an initialized struct of the specified size; job is live.
        if unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // Suspended until job membership is established, preventing a descendant-spawn race.
        command.creation_flags(0x0800_0000 | 0x0000_0004);
        Ok(tree)
    }
    impl Tree {
        pub fn attach(self, child: &Child) -> io::Result<Self> {
            // SAFETY: both handles are live. The child is still suspended.
            if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: snapshot flags are valid; returned handle is checked before use.
            let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: zero initializes a plain output struct; dwSize is set as required by ToolHelp.
            let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut resumed = false;
            // SAFETY: snapshot is live and entry is writable with its expected size.
            let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
            while more {
                if entry.th32OwnerProcessID == child.id() {
                    // SAFETY: only the suspended child's threads are opened; no inherited handles requested.
                    let thread =
                        unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                    if !thread.is_null() {
                        // SAFETY: the thread handle grants resume permission and is owned here.
                        resumed |= unsafe { ResumeThread(thread) } != u32::MAX;
                        // SAFETY: release our owned thread handle.
                        unsafe {
                            CloseHandle(thread);
                        }
                    }
                }
                // SAFETY: same valid snapshot/output struct as Thread32First.
                more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
            }
            // SAFETY: release our owned snapshot handle.
            unsafe {
                CloseHandle(snapshot);
            }
            if resumed {
                Ok(self)
            } else {
                Err(io::Error::other("Cannot resume contained child"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "subprocess fixture entered only by bounded_runner"]
    fn query_helper() {
        match std::env::var("VACANCY_TOOL_HELPER").as_deref() {
            Ok("flood") => {
                for _ in 0..200 {
                    println!("{}", "x".repeat(8192));
                    eprintln!("{}", "y".repeat(8192));
                }
            }
            Ok("wait") => std::thread::sleep(Duration::from_secs(30)),
            _ => panic!("not a tool fixture invocation"),
        }
    }
    fn helper(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "tools::tests::query_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("VACANCY_TOOL_HELPER", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    #[test]
    fn bounded_runner_drains_both_pipes_and_times_out() {
        let limits = Limits {
            timeout: Duration::from_secs(5),
            output_bytes: 1024,
        };
        let output = run(helper("flood"), limits, &CancelToken::new());
        assert_eq!(output.status, Status::Success);
        assert!(output.truncated);
        assert!(output.stdout.len() <= 1024 && output.stderr.len() <= 1024);
        let start = Instant::now();
        let output = run(
            helper("wait"),
            Limits {
                timeout: Duration::from_millis(200),
                ..limits
            },
            &CancelToken::new(),
        );
        assert_eq!(output.status, Status::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn cancel_is_first_class_and_fixture_queries_never_fall_back() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut fake = FakeRunner::default();
        assert_eq!(
            fake.query(Query::DockerContext, &cancel).status,
            Status::Cancelled
        );
        assert!(matches!(
            fake.query(Query::BrewPreview, &CancelToken::new()).status,
            Status::Refused(_)
        ));
        let mut env = Env::windows(std::path::Path::new("D:\\fixture"), "fixture");
        env.confined_to = Some(env.root.clone());
        assert!(NativeQueries::for_real(&env, Limits::default()).is_err());
    }
}
