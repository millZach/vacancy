//! The elevated reader's Windows parts: reading the volume, the pipe to the app, and checking folders against the
//! app's own account. See ADR 0007 for why each is the way it is.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    AccessCheck, GENERIC_MAPPING, GetSecurityDescriptorLength, GetTokenInformation,
    IsValidSecurityDescriptor, PRIVILEGE_SET, PSECURITY_DESCRIPTOR, RevertToSelf,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ALL_ACCESS, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED,
    FILE_FLAG_SEQUENTIAL_SCAN, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_LIST_DIRECTORY, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
    GetDriveTypeW, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SYNCHRONIZE, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, ImpersonateNamedPipeClient,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcessId, GetCurrentThread, OpenProcess, OpenProcessToken,
    OpenThreadToken, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
};

use crate::emit::{Access, Security};
use crate::mft::Disk;
use crate::wire::HELLO;

/// Closes a handle when dropped.
#[derive(Debug)]
pub struct Owned(pub HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the handle is owned here and closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

// SAFETY: a kernel handle may be used from any thread.
unsafe impl Send for Owned {}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

/// The only arguments the reader takes: which volume, and the pipe's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// The drive letter, upper case.
    pub letter: char,
    /// 32 lowercase hex digits: the pipe is `\\.\pipe\vacancy-index-<token>`.
    pub token: String,
}

/// Reads the arguments: exactly `--volume X: --pipe <32 hex digits>`. Anything else is refused.
pub fn parse_args(args: &[String]) -> Option<Args> {
    let [flag_v, volume, flag_p, token] = args else {
        return None;
    };
    if flag_v != "--volume" || flag_p != "--pipe" {
        return None;
    }
    let mut chars = volume.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    if !letter.is_ascii_uppercase() || chars.next() != Some(':') || chars.next().is_some() {
        return None;
    }
    if token.len() != 32
        || !token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return None;
    }
    Some(Args {
        letter,
        token: token.clone(),
    })
}

/// The pipe's full name for a token.
pub fn pipe_name(token: &str) -> String {
    format!(r"\\.\pipe\vacancy-index-{token}")
}

/// Whether `X:\` is a fixed or removable drive (never a network, optical or RAM drive).
pub fn is_local_drive(letter: char) -> bool {
    let root = wide(&format!("{letter}:\\"));
    // SAFETY: `root` is NUL-terminated.
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    kind == DRIVE_FIXED || kind == DRIVE_REMOVABLE
}

/// The volume, opened for reading only. Reads are made sector-aligned (4 KiB covers every sector size).
#[derive(Debug)]
pub struct VolumeDisk {
    handle: Owned,
    bounce: Vec<u8>,
}

const ALIGN: u64 = 4096;

impl VolumeDisk {
    pub fn open(letter: char) -> io::Result<Self> {
        let path = wide(&format!(r"\\.\{letter}:"));
        // SAFETY: `path` is NUL-terminated; read access only.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                FILE_READ_DATA,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_SEQUENTIAL_SCAN,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(last_error());
        }
        Ok(Self {
            handle: Owned(handle),
            bounce: Vec::new(),
        })
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let mut done = 0usize;
        while done < buf.len() {
            let at = offset + done as u64;
            let len = (buf.len() - done).min(64 << 20) as u32;
            // SAFETY: an all-zero OVERLAPPED is valid; only its offset is set, for a synchronous read.
            let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
            ov.Anonymous.Anonymous.Offset = at as u32;
            ov.Anonymous.Anonymous.OffsetHigh = (at >> 32) as u32;
            let mut read = 0u32;
            // SAFETY: `buf[done..]` holds at least `len` writable bytes; the handle is synchronous.
            let ok = unsafe {
                ReadFile(
                    self.handle.0,
                    buf[done..].as_mut_ptr(),
                    len,
                    &mut read,
                    &mut ov,
                )
            };
            if ok == 0 {
                return Err(last_error());
            }
            if read == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            done += read as usize;
        }
        Ok(())
    }
}

impl Disk for VolumeDisk {
    fn size(&mut self) -> io::Result<u64> {
        use windows_sys::Win32::System::IO::DeviceIoControl;
        use windows_sys::Win32::System::Ioctl::{
            GET_LENGTH_INFORMATION, IOCTL_DISK_GET_LENGTH_INFO,
        };
        // SAFETY: an all-zero GET_LENGTH_INFORMATION is valid.
        let mut info: GET_LENGTH_INFORMATION = unsafe { std::mem::zeroed() };
        let mut got = 0u32;
        // SAFETY: the volume is open; `info` is writable and sized as passed; synchronous.
        let ok = unsafe {
            DeviceIoControl(
                self.handle.0,
                IOCTL_DISK_GET_LENGTH_INFO,
                ptr::null(),
                0,
                (&raw mut info).cast(),
                std::mem::size_of::<GET_LENGTH_INFORMATION>() as u32,
                &mut got,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(last_error());
        }
        u64::try_from(info.Length).map_err(|_| io::Error::other("a negative length"))
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let end = offset + buf.len() as u64;
        if offset.is_multiple_of(ALIGN) && end.is_multiple_of(ALIGN) {
            return self.read_exact_at(offset, buf);
        }
        let start = offset - offset % ALIGN;
        let stop = end.div_ceil(ALIGN) * ALIGN;
        let mut bounce = std::mem::take(&mut self.bounce);
        bounce.resize((stop - start) as usize, 0);
        let result = self.read_exact_at(start, &mut bounce);
        if result.is_ok() {
            let from = (offset - start) as usize;
            buf.copy_from_slice(&bounce[from..from + buf.len()]);
        }
        self.bounce = bounce;
        result
    }
}

/// Checks folders against the app's account: its token, captured from the pipe at identification level (enough
/// to check access, never to act as it).
#[derive(Debug)]
pub struct TokenAccess {
    token: Owned,
    broken: bool,
    bypass: bool,
}

impl TokenAccess {
    pub fn new(token: Owned) -> Self {
        let bypass = bypasses_traverse(token.0);
        Self {
            token,
            broken: false,
            bypass,
        }
    }
}

impl TokenAccess {
    /// Whether the token may do `desired` to a folder with descriptor `security`.
    fn allows(&mut self, security: Security<'_>, desired: u32) -> bool {
        let Security::Descriptor(sd) = security else {
            return false;
        };
        // The descriptor came off the disk, and Windows follows its offsets without a length: check it is whole
        // first, in Rust.
        if !crate::sd::is_well_formed(sd) {
            return false;
        }
        // A copy, 4-byte aligned as Windows expects a descriptor to be.
        let mut copy = vec![0u32; sd.len().div_ceil(4)];
        // SAFETY: `copy` holds at least `sd.len()` bytes, and they don't overlap `sd`.
        unsafe { ptr::copy_nonoverlapping(sd.as_ptr(), copy.as_mut_ptr().cast::<u8>(), sd.len()) };
        let psd: PSECURITY_DESCRIPTOR = copy.as_mut_ptr().cast();
        // SAFETY: checked above to be a self-relative descriptor wholly inside `copy`; these calls only read it.
        let valid = unsafe { IsValidSecurityDescriptor(psd) } != 0
            && (unsafe { GetSecurityDescriptorLength(psd) } as usize) <= sd.len();
        if !valid {
            return false;
        }
        let mapping = GENERIC_MAPPING {
            GenericRead: FILE_GENERIC_READ,
            GenericWrite: FILE_GENERIC_WRITE,
            GenericExecute: FILE_GENERIC_EXECUTE,
            GenericAll: FILE_ALL_ACCESS,
        };
        // Room for the privileges AccessCheck may report (a fixed PRIVILEGE_SET holds one).
        let mut privileges = vec![0u64; 64];
        let mut privileges_len = (privileges.len() * 8) as u32;
        let mut granted = 0u32;
        let mut status = 0i32;
        // SAFETY: every pointer is valid for the call; the descriptor was checked above; `privileges` is 8-byte
        // aligned and `privileges_len` long; the token is an identification-level token.
        let ok = unsafe {
            AccessCheck(
                psd,
                self.token.0,
                desired,
                &mapping,
                privileges.as_mut_ptr().cast::<PRIVILEGE_SET>(),
                &mut privileges_len,
                &mut granted,
                &mut status,
            )
        };
        if ok == 0 {
            // A descriptor Windows won't take is this folder's problem (locked); anything else means checks can't
            // be made at all (a bad token), and the whole read stops.
            const ERROR_INVALID_ACL: u32 = 1336;
            const ERROR_INVALID_SID: u32 = 1337;
            const ERROR_INVALID_SECURITY_DESCR: u32 = 1338;
            // SAFETY: reads this thread's last error.
            let code = unsafe { GetLastError() };
            if !matches!(
                code,
                ERROR_INVALID_ACL | ERROR_INVALID_SID | ERROR_INVALID_SECURITY_DESCR
            ) {
                self.broken = true;
            }
            return false;
        }
        status != 0
    }
}

impl Access for TokenAccess {
    fn can_list(&mut self, security: Security<'_>) -> bool {
        self.allows(security, FILE_LIST_DIRECTORY | SYNCHRONIZE)
    }

    fn can_traverse(&mut self, security: Security<'_>) -> bool {
        self.allows(security, FILE_TRAVERSE | SYNCHRONIZE)
    }

    fn bypasses_traverse(&self) -> bool {
        self.bypass
    }

    fn broken(&self) -> bool {
        self.broken
    }
}

/// Whether `token` has `SeChangeNotifyPrivilege` ("bypass traverse checking") enabled. If it can't be told, no:
/// then every folder above one is checked, which can only lock more.
fn bypasses_traverse(token: HANDLE) -> bool {
    use windows_sys::Win32::Foundation::LUID;
    use windows_sys::Win32::Security::{
        LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TokenPrivileges,
    };
    let name = wide("SeChangeNotifyPrivilege");
    // SAFETY: an all-zero LUID is valid.
    let mut luid: LUID = unsafe { std::mem::zeroed() };
    // SAFETY: `name` is NUL-terminated; `luid` is writable.
    if unsafe { LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &mut luid) } == 0 {
        return false;
    }
    let mut buf = vec![0u64; 512];
    let mut len = 0u32;
    // SAFETY: `buf` is 4 KiB of writable, 8-byte aligned memory.
    if unsafe {
        GetTokenInformation(
            token,
            TokenPrivileges,
            buf.as_mut_ptr().cast(),
            (buf.len() * 8) as u32,
            &mut len,
        )
    } == 0
    {
        return false;
    }
    let bytes = &buf[..];
    let count = (bytes[0] & 0xFFFF_FFFF) as usize;
    let entry = std::mem::size_of::<LUID_AND_ATTRIBUTES>();
    if 4 + count * entry > (len as usize).min(buf.len() * 8) {
        return false;
    }
    // SAFETY: TOKEN_PRIVILEGES is a count followed by `count` LUID_AND_ATTRIBUTES, all inside `buf` (checked).
    let entries = unsafe {
        std::slice::from_raw_parts(
            buf.as_ptr()
                .cast::<u8>()
                .add(4)
                .cast::<LUID_AND_ATTRIBUTES>(),
            count,
        )
    };
    entries.iter().any(|e| {
        e.Luid.LowPart == luid.LowPart
            && e.Luid.HighPart == luid.HighPart
            && e.Attributes & SE_PRIVILEGE_ENABLED != 0
    })
}

/// The pipe, the reader's end: created by the reader, connected to by the app.
#[derive(Debug)]
pub struct Pipe {
    handle: Owned,
    event: Owned,
}

impl Pipe {
    /// Creates `\\.\pipe\vacancy-index-<token>`: the first and only instance (a name someone else made first
    /// fails), local clients only, and open to the parent process's account alone, at medium integrity.
    pub fn create(token: &str, parent_user_sid: &str) -> io::Result<Self> {
        // Read and write data, read attributes, synchronize: no right to make more instances.
        let sddl = wide(&format!(
            "D:P(A;;0x120083;;;{parent_user_sid})S:(ML;;NW;;;ME)"
        ));
        let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; `sd` receives a LocalAlloc'd descriptor, freed below.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let name = wide(&pipe_name(token));
        // SAFETY: `name` is NUL-terminated and `attrs` points at a valid descriptor for the call.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                1 << 20,
                64,
                0,
                &attrs,
            )
        };
        // SAFETY: `sd` came from LocalAlloc.
        unsafe { LocalFree(sd) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(last_error());
        }
        // SAFETY: a manual-reset, unnamed event.
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if event.is_null() {
            return Err(last_error());
        }
        Ok(Self {
            handle: Owned(handle),
            event: Owned(event),
        })
    }

    /// Waits up to `timeout` for the app to connect, then checks that it is the process that started this one.
    pub fn accept(&self, timeout: Duration, expected_pid: u32) -> io::Result<()> {
        let mut ov = self.overlapped();
        // SAFETY: the pipe and event are open; `ov` lives until the wait below is over.
        let ok = unsafe { ConnectNamedPipe(self.handle.0, &mut ov) };
        if ok == 0 {
            // SAFETY: reads this thread's last error.
            match unsafe { GetLastError() } {
                ERROR_PIPE_CONNECTED => {}
                ERROR_IO_PENDING => {
                    self.finish(&mut ov, timeout)?;
                }
                _ => return Err(last_error()),
            }
        }
        let mut pid = 0u32;
        // SAFETY: the pipe is connected; `pid` is writable.
        if unsafe { GetNamedPipeClientProcessId(self.handle.0, &mut pid) } == 0 {
            return Err(last_error());
        }
        if pid != expected_pid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "another process connected",
            ));
        }
        Ok(())
    }

    /// Reads the app's hello (and nothing else), then captures the app's token at identification level.
    pub fn hello_and_token(&self, timeout: Duration) -> io::Result<Owned> {
        let mut got = [0u8; HELLO.len()];
        let mut at = 0;
        while at < got.len() {
            let n = self.read(&mut got[at..], timeout)?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            at += n;
        }
        if got != HELLO {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not the app's hello",
            ));
        }
        // SAFETY: the pipe is connected and has been read from, as impersonation requires.
        if unsafe { ImpersonateNamedPipeClient(self.handle.0) } == 0 {
            return Err(last_error());
        }
        let mut token: HANDLE = ptr::null_mut();
        // SAFETY: opens this thread's impersonation token, checked as the reader itself (OpenAsSelf).
        let ok = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) };
        let err = last_error();
        // SAFETY: always revert, whatever happened.
        let reverted = unsafe { RevertToSelf() } != 0;
        if !reverted {
            // Never carry on as anyone else.
            std::process::exit(3);
        }
        if ok == 0 {
            return Err(err);
        }
        Ok(Owned(token))
    }

    /// Waits until the app has read everything sent (or has gone).
    pub fn wait_drained(&self) {
        use windows_sys::Win32::Storage::FileSystem::FlushFileBuffers;
        // SAFETY: the pipe is open.
        unsafe { FlushFileBuffers(self.handle.0) };
    }

    fn overlapped(&self) -> OVERLAPPED {
        // SAFETY: an all-zero OVERLAPPED is valid.
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = self.event.0;
        ov
    }

    /// Waits for the operation on `ov`; cancels it after `timeout`.
    fn finish(&self, ov: &mut OVERLAPPED, timeout: Duration) -> io::Result<u32> {
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the event belongs to `ov`, which is pending on this pipe.
        if unsafe { WaitForSingleObject(self.event.0, ms) } != WAIT_OBJECT_0 {
            // SAFETY: cancels this handle's pending operation; then wait for it to end.
            unsafe { CancelIoEx(self.handle.0, ov) };
            let mut n = 0u32;
            // SAFETY: as above.
            unsafe { GetOverlappedResult(self.handle.0, ov, &mut n, 1) };
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut n = 0u32;
        // SAFETY: the operation on `ov` has completed.
        if unsafe { GetOverlappedResult(self.handle.0, ov, &mut n, 0) } == 0 {
            return Err(last_error());
        }
        Ok(n)
    }

    fn read(&self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        let mut ov = self.overlapped();
        let mut n = 0u32;
        // SAFETY: `buf` is writable for its length and outlives the operation (waited for below).
        let ok = unsafe {
            ReadFile(
                self.handle.0,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut n,
                &mut ov,
            )
        };
        if ok != 0 {
            return Ok(n as usize);
        }
        // SAFETY: reads this thread's last error.
        if unsafe { GetLastError() } != ERROR_IO_PENDING {
            return Err(last_error());
        }
        Ok(self.finish(&mut ov, timeout)? as usize)
    }
}

impl io::Write for &Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut ov = self.overlapped();
        let mut n = 0u32;
        let len = buf.len().min(1 << 20) as u32;
        // SAFETY: `buf` is readable for `len` bytes and outlives the operation (waited for below).
        let ok = unsafe { WriteFile(self.handle.0, buf.as_ptr(), len, &mut n, &mut ov) };
        if ok != 0 {
            return Ok(n as usize);
        }
        // SAFETY: reads this thread's last error.
        if unsafe { GetLastError() } != ERROR_IO_PENDING {
            return Err(last_error());
        }
        // A reader that stops reading for a minute has gone: give up.
        Ok(self.finish(&mut ov, Duration::from_secs(60))? as usize)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The process that started this one: its id, its account's SID as a string, and a handle on it, held for as long
/// as the reader runs. While a process is held its id can't be given to another, so the pipe's client check
/// ([`Pipe::accept`]) can't be met by a newer process that got the id; and the reader exits when it ends
/// ([`exit_with`]). A process younger than this one isn't its parent (the id was reused): refused.
pub fn parent() -> io::Result<(u32, String, Owned)> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: a snapshot of the process list.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return Err(last_error());
    }
    let snap = Owned(snap);
    // SAFETY: an all-zero PROCESSENTRY32W is valid once its size is set.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: reads this process's id.
    let me = unsafe { GetCurrentProcessId() };
    let mut parent = None;
    // SAFETY: `entry` is sized; the snapshot is open.
    let mut ok = unsafe { Process32FirstW(snap.0, &mut entry) };
    while ok != 0 {
        if entry.th32ProcessID == me {
            parent = Some(entry.th32ParentProcessID);
            break;
        }
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap.0, &mut entry) };
    }
    let pid = parent.ok_or_else(|| io::Error::other("no parent process"))?;
    // SAFETY: opens the parent for its token, its times, and waiting on it.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        return Err(last_error());
    }
    let process = Owned(process);
    // SAFETY: reads a pseudo-handle for this process.
    let me_handle = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() };
    match (created(process.0), created(me_handle)) {
        (Some(parent_at), Some(me_at)) if parent_at <= me_at => {}
        _ => {
            return Err(io::Error::other(
                "the parent process isn't older than the reader",
            ));
        }
    }
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the process handle is open.
    if unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last_error());
    }
    let token = Owned(token);
    let mut buf = vec![0u64; 64];
    let mut len = 0u32;
    // SAFETY: `buf` is 512 writable, aligned bytes.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buf.as_mut_ptr().cast(),
            (buf.len() * 8) as u32,
            &mut len,
        )
    } == 0
    {
        return Err(last_error());
    }
    // SAFETY: GetTokenInformation filled `buf` with a TOKEN_USER.
    let user = unsafe { &*buf.as_ptr().cast::<TOKEN_USER>() };
    let mut s: windows_sys::core::PWSTR = ptr::null_mut();
    // SAFETY: the SID is inside `buf`; `s` receives a LocalAlloc'd string, freed below.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut s) } == 0 {
        return Err(last_error());
    }
    // SAFETY: `s` is a NUL-terminated string.
    let sid = unsafe {
        let mut n = 0;
        while *s.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(s, n))
    };
    // SAFETY: `s` came from LocalAlloc.
    unsafe { LocalFree(s.cast()) };
    if !sid.starts_with("S-1-")
        || !sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S')
    {
        return Err(io::Error::other("an unexpected SID"));
    }
    Ok((pid, sid, process))
}

/// When a process was created (FILETIME ticks).
fn created(process: HANDLE) -> Option<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;
    // SAFETY: all-zero FILETIMEs are valid.
    let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) =
        unsafe { std::mem::zeroed() };
    // SAFETY: the handle has query access; the outputs are writable.
    let ok = unsafe { GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u) };
    (ok != 0).then(|| (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime))
}

/// Exits this process with `code` as soon as `process` ends, from a thread of its own that holds it.
pub fn exit_with(process: Owned, code: i32) {
    std::thread::spawn(move || {
        let process = process;
        // SAFETY: the handle has SYNCHRONIZE access and is held by this thread.
        unsafe { WaitForSingleObject(process.0, u32::MAX) };
        std::process::exit(code);
    });
}

/// Loads DLLs from System32 only, from now on (the reader's folder may be writable by the user).
pub fn harden_dll_search() {
    use windows_sys::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
    };
    // SAFETY: changes this process's DLL search path only.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn takes_only_a_drive_letter_and_a_pipe_token() {
        let token = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            parse_args(&args(&["--volume", "c:", "--pipe", token])),
            Some(Args {
                letter: 'C',
                token: token.into()
            })
        );
        for bad in [
            vec!["--volume", "C:\\", "--pipe", token],
            vec!["--volume", "CC:", "--pipe", token],
            vec!["--volume", "1:", "--pipe", token],
            vec!["--volume", r"\\.\C:", "--pipe", token],
            vec![
                "--volume",
                "C:",
                "--pipe",
                "0123456789ABCDEF0123456789ABCDEF",
            ],
            vec!["--volume", "C:", "--pipe", "short"],
            vec![
                "--volume",
                "C:",
                "--pipe",
                "../../../../x23456789abcdef01234",
            ],
            vec!["--pipe", token, "--volume", "C:"],
            vec!["--volume", "C:", "--pipe", token, "--extra"],
            vec![],
        ] {
            assert_eq!(parse_args(&args(&bad)), None, "{bad:?}");
        }
    }
}
