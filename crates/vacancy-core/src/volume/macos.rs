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

pub(super) fn file_system_of(path: &Path) -> String {
    volume_info(path).map(|v| v.file_system).unwrap_or_default()
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

/// What macOS says about one mounted file system. Its name and device are asked for only when it might be offered
/// (and always for `/`): a stale network share can block `getattrlist` for a long time.
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
    let file_system = fs.to_string_lossy().into_owned();
    let ask = mount_point == "/" || rules::worth_asking(local, &file_system);
    let block = u64::from(st.f_bsize);
    let device = match arbitration {
        Some(a) if ask => a.device(from),
        _ => Device::default(),
    };
    let name = if ask { volume_name(on) } else { None };
    // SAFETY: `fsid_t` is two `i32`s (its field is private in `libc`); transmute checks the sizes match.
    let fsid = unsafe { mem::transmute::<libc::fsid_t, [i32; 2]>(st.f_fsid) };
    MacMount {
        name: name.unwrap_or_else(|| fallback_name(&mount_point)),
        fsid,
        mount_point,
        file_system,
        local,
        browsable: flags & libc::MNT_DONTBROWSE as u32 == 0,
        read_only: flags & libc::MNT_RDONLY as u32 != 0,
        internal: device.internal,
        protocol: device.protocol,
        role: device.role,
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

// ---------- Disk Arbitration and IOKit: is the device built in, and what is the volume for? ----------

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFAllocatorRef = *const c_void;
type DASessionRef = *const c_void;
type DADiskRef = *const c_void;
type CFIndex = isize;
type CFTypeID = usize;
/// `io_object_t`: a Mach port naming an IOKit object; 0 is none.
type IoObject = u32;
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
    fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        s: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFArrayGetTypeID() -> CFTypeID;
    fn CFArrayGetCount(array: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: CFIndex) -> *const c_void;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegistryEntryCreateCFProperty(
        entry: IoObject,
        key: CFStringRef,
        allocator: CFAllocatorRef,
        options: u32,
    ) -> CFTypeRef;
    fn IOObjectRelease(object: IoObject) -> i32;
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
    fn DADiskCopyIOMedia(disk: DADiskRef) -> IoObject;
    static kDADiskDescriptionDeviceInternalKey: CFStringRef;
    static kDADiskDescriptionDeviceProtocolKey: CFStringRef;
}

/// What Disk Arbitration and IOKit say about the device under a volume, and the volume's APFS role.
#[derive(Default)]
struct Device {
    internal: Option<bool>,
    protocol: Option<String>,
    role: Option<String>,
}

/// A Disk Arbitration session, released when dropped. Only asks; it never mounts, unmounts or claims anything.
struct Arbitration(DASessionRef);

impl Arbitration {
    fn new() -> Option<Self> {
        // SAFETY: a null allocator means the default one.
        let session = unsafe { DASessionCreate(ptr::null()) };
        (!session.is_null()).then_some(Self(session))
    }

    /// The device behind `/dev/diskNsM`: whether it is built in, how it is attached, and the volume's APFS role.
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
        // SAFETY: as above.
        let role = unsafe { apfs_role(disk) };
        // SAFETY: we own `disk` (a Create call) and are done with it.
        unsafe { CFRelease(disk) };
        if description.is_null() {
            return Device {
                role,
                ..Device::default()
            };
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
                role,
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

/// An APFS volume's role ("Preboot", "Recovery", "VM", "Data"): the first entry of the `Role` property on the
/// volume's IOMedia, as `diskutil apfs list` shows it. `None` when it has no role or isn't APFS.
///
/// # Safety
/// `disk` is a live DADisk.
unsafe fn apfs_role(disk: DADiskRef) -> Option<String> {
    // SAFETY: the caller promises `disk` is live; the media object we get is ours to release.
    let media = unsafe { DADiskCopyIOMedia(disk) };
    if media == 0 {
        return None;
    }
    // SAFETY: a NUL-terminated literal; a null allocator means the default one.
    let key = unsafe { CFStringCreateWithCString(ptr::null(), c"Role".as_ptr(), UTF8) };
    let value = if key.is_null() {
        ptr::null()
    } else {
        // SAFETY: `media` and `key` are live; a null allocator means the default one. The value is ours to release.
        let value = unsafe { IORegistryEntryCreateCFProperty(media, key, ptr::null(), 0) };
        // SAFETY: we own `key` (a Create call) and are done with it.
        unsafe { CFRelease(key) };
        value
    };
    // SAFETY: we own `media` (a Copy call) and are done with it.
    unsafe { IOObjectRelease(media) };
    if value.is_null() {
        return None;
    }
    // SAFETY: `value` is a live CF object we own; types are checked before each read, and the array's items are read
    // before it is released.
    let role = unsafe {
        if CFGetTypeID(value) == CFArrayGetTypeID() {
            if CFArrayGetCount(value) > 0 {
                cf_string(CFArrayGetValueAtIndex(value, 0))
            } else {
                None
            }
        } else {
            cf_string(value)
        }
    };
    // SAFETY: we own `value` (a Create call) and are done with it.
    unsafe { CFRelease(value) };
    role.filter(|r| !r.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only: asks macOS about its own Preboot and VM volumes, as it mounts them at startup, so the role really is
    /// readable where the rules rely on it.
    #[test]
    fn reads_the_apfs_role_of_the_systems_own_volumes() {
        let arbitration = Arbitration::new().expect("a Disk Arbitration session");
        let mut checked = 0;
        for (path, role) in [
            ("/System/Volumes/Preboot", "Preboot"),
            ("/System/Volumes/VM", "VM"),
        ] {
            if !Path::new(path).exists() {
                continue;
            }
            let c_path = CString::new(path).expect("no NUL");
            let mut st = MaybeUninit::<libc::statfs>::uninit();
            // SAFETY: `c_path` is NUL-terminated; `st` is writable memory for one `statfs`.
            assert_eq!(unsafe { libc::statfs(c_path.as_ptr(), st.as_mut_ptr()) }, 0);
            // SAFETY: `statfs` returned 0, so it filled `st`.
            let st = unsafe { st.assume_init() };
            let m = mount(&st, Some(&arbitration));
            if m.mount_point != path {
                continue; // not a volume of its own on this Mac
            }
            assert_eq!(m.role.as_deref(), Some(role), "{m:?}");
            assert!(
                !rules::drives(std::slice::from_ref(&m))
                    .iter()
                    .any(|d| d.mount_point == path)
            );
            checked += 1;
        }
        assert!(checked > 0, "found neither Preboot nor VM to ask about");
    }
}
