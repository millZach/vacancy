//! macOS: the mounted file systems from `getfsstat` (or one from `statfs`), the volume's name ("Macintosh HD") from
//! `getattrlist`, and whether its device is built in from Disk Arbitration. [`super::offer::macos`] decides what to
//! offer.
//!
//! On APFS the sizes are the container's, which is what Finder and System Settings show.

use std::ffi::{CStr, CString, c_char, c_void};
use std::io;
use std::mem::{self, MaybeUninit};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use super::offer::macos::{self as rules, MacMount};
use super::{VolumeError, VolumeInfo};

pub(super) fn volume_info(path: &Path) -> Result<VolumeInfo, VolumeError> {
    let fail = |source| VolumeError::Io {
        path: path.to_path_buf(),
        source,
    };
    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        fail(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path contains a NUL byte",
        ))
    })?;

    let mut st = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `c_path` is NUL-terminated; `st` is writable memory for one `statfs`.
    if unsafe { libc::statfs(c_path.as_ptr(), st.as_mut_ptr()) } != 0 {
        return Err(fail(io::Error::last_os_error()));
    }
    // SAFETY: `statfs` returned 0, so it filled `st`.
    let st = unsafe { st.assume_init() };
    Ok(rules::describe(&mount(&st, Arbitration::new().as_ref())))
}

pub(super) fn system_volume() -> Result<VolumeInfo, VolumeError> {
    super::volume_info(Path::new("/"))
}

pub(super) fn list_volumes() -> Result<Vec<VolumeInfo>, VolumeError> {
    // SAFETY: a null buffer asks only for the number of mounts.
    let n = unsafe { libc::getfsstat(ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if n < 0 {
        return Err(VolumeError::List(io::Error::last_os_error()));
    }
    // Room for a few more, in case something is mounted between the two calls.
    let cap = n as usize + 8;
    let mut buf: Vec<libc::statfs> = Vec::with_capacity(cap);
    let bytes = (cap * mem::size_of::<libc::statfs>()) as libc::c_int;
    // SAFETY: `buf` has room for `cap` statfs records, `bytes` long; getfsstat writes at most that many.
    let got = unsafe { libc::getfsstat(buf.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
    if got < 0 {
        return Err(VolumeError::List(io::Error::last_os_error()));
    }
    // SAFETY: getfsstat filled the first `got` records.
    unsafe { buf.set_len((got as usize).min(cap)) };
    let arbitration = Arbitration::new();
    let found: Vec<MacMount> = buf
        .iter()
        .map(|st| mount(st, arbitration.as_ref()))
        .collect();
    Ok(rules::drives(&found))
}

/// What macOS says about one mounted file system. Disk Arbitration is asked only about local devices.
fn mount(st: &libc::statfs, arbitration: Option<&Arbitration>) -> MacMount {
    // SAFETY: the kernel NUL-terminates the three names within their arrays.
    let (on, from, fs) = unsafe {
        (
            CStr::from_ptr(st.f_mntonname.as_ptr()),
            CStr::from_ptr(st.f_mntfromname.as_ptr()),
            CStr::from_ptr(st.f_fstypename.as_ptr()),
        )
    };
    let flags = st.f_flags;
    let local = flags & libc::MNT_LOCAL as u32 != 0;
    let mount_point = on.to_string_lossy().into_owned();
    let block = u64::from(st.f_bsize);
    let device = match arbitration {
        Some(a) if local => a.device(from),
        _ => Device::default(),
    };
    MacMount {
        name: volume_name(on).unwrap_or_else(|| fallback_name(&mount_point)),
        mount_point,
        file_system: fs.to_string_lossy().into_owned(),
        local,
        browsable: flags & libc::MNT_DONTBROWSE as u32 == 0,
        read_only: flags & libc::MNT_RDONLY as u32 != 0,
        internal: device.internal,
        protocol: device.protocol,
        total_bytes: st.f_blocks.saturating_mul(block),
        free_bytes: st.f_bfree.saturating_mul(block),
    }
}

const NAME_BUF_LEN: usize = 4 + 8 + 1024;

/// The buffer `getattrlist` fills: a `u32` total length, an `attrreference_t` (offset, length), then the name.
#[repr(C, align(8))]
struct NameBuf([u8; NAME_BUF_LEN]);

/// The volume's name as Finder shows it, from `getattrlist(ATTR_VOL_NAME)` on its mount point.
fn volume_name(mount: &CStr) -> Option<String> {
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_NAME,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut buf = NameBuf([0; NAME_BUF_LEN]);
    // SAFETY: `mount` is NUL-terminated; `list` and `buf` live for the whole call and `buf` is
    // `size_of::<NameBuf>()` writable bytes.
    let rc = unsafe {
        libc::getattrlist(
            mount.as_ptr(),
            (&raw mut list).cast(),
            (&raw mut buf).cast(),
            mem::size_of::<NameBuf>(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let bytes = &buf.0;
    let word = |at: usize| bytes.get(at..at + 4).map(|b| [b[0], b[1], b[2], b[3]]);
    // The data offset counts from the attrreference_t itself, which starts after the leading length.
    let offset = usize::try_from(i32::from_ne_bytes(word(4)?)).ok()?;
    let length = u32::from_ne_bytes(word(8)?) as usize;
    let start = 4usize.checked_add(offset)?;
    let raw = bytes.get(start..start.checked_add(length)?)?;
    let raw = raw.split(|&b| b == 0).next()?;
    let name = String::from_utf8_lossy(raw).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// Used when the OS won't name the volume: the last folder of the mount point.
fn fallback_name(mount_point: &str) -> String {
    match Path::new(mount_point).file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => mount_point.to_owned(),
    }
}

// ---------- Disk Arbitration: is the device built in? ----------

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFAllocatorRef = *const c_void;
type DASessionRef = *const c_void;
type DADiskRef = *const c_void;
type CFIndex = isize;
type CFTypeID = usize;
const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    fn CFBooleanGetTypeID() -> CFTypeID;
    fn CFStringGetTypeID() -> CFTypeID;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: CFIndex, encoding: u32) -> u8;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
}

#[link(name = "DiskArbitration", kind = "framework")]
unsafe extern "C" {
    fn DASessionCreate(allocator: CFAllocatorRef) -> DASessionRef;
    fn DADiskCreateFromBSDName(
        allocator: CFAllocatorRef,
        session: DASessionRef,
        name: *const c_char,
    ) -> DADiskRef;
    fn DADiskCopyDescription(disk: DADiskRef) -> CFDictionaryRef;
    static kDADiskDescriptionDeviceInternalKey: CFStringRef;
    static kDADiskDescriptionDeviceProtocolKey: CFStringRef;
}

/// What Disk Arbitration says about the device under a volume.
#[derive(Default)]
struct Device {
    internal: Option<bool>,
    protocol: Option<String>,
}

/// A Disk Arbitration session, released when dropped. Only asks; it never mounts, unmounts or claims anything.
struct Arbitration(DASessionRef);

impl Arbitration {
    fn new() -> Option<Self> {
        // SAFETY: a null allocator means the default one.
        let session = unsafe { DASessionCreate(ptr::null()) };
        (!session.is_null()).then_some(Self(session))
    }

    /// The device behind `/dev/diskNsM`: whether it is built in, and how it is attached.
    fn device(&self, from: &CStr) -> Device {
        let Some(bsd) = from.to_bytes().strip_prefix(b"/dev/") else {
            return Device::default();
        };
        let Ok(bsd) = CString::new(bsd) else {
            return Device::default();
        };
        // SAFETY: the session is live, `bsd` is NUL-terminated, and a null allocator means the default one.
        let disk = unsafe { DADiskCreateFromBSDName(ptr::null(), self.0, bsd.as_ptr()) };
        if disk.is_null() {
            return Device::default();
        }
        // SAFETY: `disk` is a live DADisk we own.
        let description = unsafe { DADiskCopyDescription(disk) };
        // SAFETY: we own `disk` (a Create call) and are done with it.
        unsafe { CFRelease(disk) };
        if description.is_null() {
            return Device::default();
        }
        // SAFETY: `description` is a live dictionary; the keys are Disk Arbitration's constants. Get returns values
        // the dictionary still owns, read before it is released.
        let device = unsafe {
            Device {
                internal: cf_bool(CFDictionaryGetValue(
                    description,
                    kDADiskDescriptionDeviceInternalKey,
                )),
                protocol: cf_string(CFDictionaryGetValue(
                    description,
                    kDADiskDescriptionDeviceProtocolKey,
                )),
            }
        };
        // SAFETY: we own `description` (a Copy call) and are done with it.
        unsafe { CFRelease(description) };
        device
    }
}

impl Drop for Arbitration {
    fn drop(&mut self) {
        // SAFETY: we own the session (a Create call), and it is released once.
        unsafe { CFRelease(self.0) };
    }
}

/// A CFBoolean's value; `None` for null or anything else.
///
/// # Safety
/// `value` is null or a live Core Foundation object.
unsafe fn cf_bool(value: *const c_void) -> Option<bool> {
    // SAFETY: the caller promises `value` is null or a live CF object; the type is checked before the read.
    unsafe {
        (!value.is_null() && CFGetTypeID(value) == CFBooleanGetTypeID())
            .then(|| CFBooleanGetValue(value) != 0)
    }
}

/// A CFString as UTF-8; `None` for null, anything else, or a string too long for the buffer.
///
/// # Safety
/// `value` is null or a live Core Foundation object.
unsafe fn cf_string(value: *const c_void) -> Option<String> {
    let mut buf = [0 as c_char; 256];
    // SAFETY: the caller promises `value` is null or a live CF object; the type is checked before the read, and
    // `buf` holds `buf.len()` writable bytes.
    let ok = unsafe {
        !value.is_null()
            && CFGetTypeID(value) == CFStringGetTypeID()
            && CFStringGetCString(value, buf.as_mut_ptr(), buf.len() as CFIndex, UTF8) != 0
    };
    if !ok {
        return None;
    }
    // SAFETY: CFStringGetCString succeeded, so `buf` holds a NUL-terminated string.
    let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
    Some(s.to_string_lossy().into_owned())
}
