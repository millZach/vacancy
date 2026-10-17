//! This computer's name, as the OS shows it to people: "DESKTOP-ALEX", "Alex's MacBook Air".

/// The computer's name for display, or "This computer" when the OS won't say.
pub fn machine_name() -> String {
    imp::machine_name()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "This computer".to_owned())
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use windows_sys::Win32::System::SystemInformation::{
        ComputerNamePhysicalDnsHostname, GetComputerNameExW,
    };

    /// The device name from Settings > System > About, with its original case.
    pub(super) fn machine_name() -> Option<String> {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` holds `len` writable UTF-16 units; on success Windows sets `len` to the units written.
        let ok = unsafe {
            GetComputerNameExW(ComputerNamePhysicalDnsHostname, buf.as_mut_ptr(), &mut len)
        };
        if ok == 0 {
            return std::env::var("COMPUTERNAME").ok();
        }
        let len = (len as usize).min(buf.len());
        Some(
            OsString::from_wide(&buf[..len])
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{c_char, c_void};
    use std::ptr;

    type CFStringRef = *const c_void;
    type CFIndex = isize;
    const UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8

    #[link(name = "SystemConfiguration", kind = "framework")]
    unsafe extern "C" {
        fn SCDynamicStoreCopyComputerName(store: *const c_void, encoding: *mut u32) -> CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringGetLength(string: CFStringRef) -> CFIndex;
        fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
        fn CFStringGetCString(
            string: CFStringRef,
            buffer: *mut c_char,
            size: CFIndex,
            encoding: u32,
        ) -> u8;
        fn CFRelease(object: *const c_void);
    }

    /// The Computer Name from System Settings > General > Sharing.
    pub(super) fn machine_name() -> Option<String> {
        // SAFETY: a null store asks for a one-off session, and the encoding out pointer may be null. The result
        // follows the Copy rule, so it is released below.
        let name = unsafe { SCDynamicStoreCopyComputerName(ptr::null(), ptr::null_mut()) };
        if name.is_null() {
            return None;
        }
        // SAFETY: `name` is a live CFString until the CFRelease below.
        let size = unsafe { CFStringGetMaximumSizeForEncoding(CFStringGetLength(name), UTF8) }
            .saturating_add(1);
        let mut buf = vec![0u8; usize::try_from(size).unwrap_or(0).max(1)];
        // SAFETY: `buf` holds `size` writable bytes; CFStringGetCString NUL-terminates on success.
        let ok = unsafe { CFStringGetCString(name, buf.as_mut_ptr().cast(), size, UTF8) };
        // SAFETY: `name` came from a Copy function and is not used after this.
        unsafe { CFRelease(name) };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(end);
        String::from_utf8(buf).ok()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub(super) fn machine_name() -> Option<String> {
        None
    }
}
