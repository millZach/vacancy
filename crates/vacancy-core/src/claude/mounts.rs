//! Every file system mounted now, by each place it is mounted, as the OS reports it (ADR 0008): the list privacy mode
//! off's deny rules are built from ([`super::runner::mount_rules_in`]), read again right before each call. It doesn't
//! depend on what the drive picker offers: a read-only disk image, a snapshot, a share, a VHD in an empty folder and
//! the system's own volumes are all in it.
//!
//! - **macOS:** `getfsstat`, every mount's `f_mntonname` (`MNT_NOWAIT`: the kernel's list as it stands, without
//!   asking a slow share to refresh its figures).
//! - **Windows:** every volume from `FindFirstVolumeW`/`FindNextVolumeW`, and each of its paths from
//!   `GetVolumePathNamesForVolumeNameW`: its drive letter, if any, and every empty folder it is mounted in.
//!
//! A list that can't be read whole is an error: the caller then runs the call as if privacy mode were on.

use std::path::PathBuf;

/// Every place a file system is mounted now. An error, in words, when the OS won't say, or says something Vacancy
/// can't take as a path (not Unicode, or a mount point the kernel may have cut short).
pub fn list() -> Result<Vec<PathBuf>, String> {
    imp::list().map_err(|why| {
        format!(
            "Claude can't open files for now: Vacancy couldn't read which drives and disk images are mounted \
             ({why}), so it can't keep Claude off them. Claude gets names, sizes, counts and dates only."
        )
    })
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::path::PathBuf;
    use std::{io, mem, ptr};

    pub(super) fn list() -> Result<Vec<PathBuf>, String> {
        // SAFETY: a null buffer asks only for the number of mounts.
        let n = unsafe { libc::getfsstat(ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        if n < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        // Room for more, in case something is mounted between the two calls; a full buffer means it may not be all.
        let cap = n as usize + 16;
        let mut buf: Vec<libc::statfs> = Vec::with_capacity(cap);
        let bytes = (cap * mem::size_of::<libc::statfs>()) as libc::c_int;
        // SAFETY: `buf` has room for `cap` statfs records, `bytes` long; getfsstat writes at most that many.
        let got = unsafe { libc::getfsstat(buf.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
        if got < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        if got as usize >= cap {
            return Err("more file systems were mounted while Vacancy listed them".into());
        }
        // SAFETY: getfsstat filled the first `got` records.
        unsafe { buf.set_len(got as usize) };
        let mut out = Vec::with_capacity(buf.len());
        for st in &buf {
            let room = st.f_mntonname.len();
            // SAFETY: the kernel NUL-terminates the name within its array.
            let on = unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) };
            if on.to_bytes().len() + 1 >= room {
                return Err("a mount point is too long for the kernel to give whole".into());
            }
            let Ok(text) = on.to_str() else {
                return Err(format!(
                    "the mount point {} isn't Unicode",
                    on.to_string_lossy()
                ));
            };
            out.push(PathBuf::from(text));
        }
        Ok(out)
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::{
        ERROR_MORE_DATA, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetVolumePathNamesForVolumeNameW,
    };

    /// `\\?\Volume{GUID}\` and its NUL fit in 50 units; the API's own examples use `MAX_PATH`.
    const VOLUME_NAME_LEN: usize = 261;

    pub(super) fn list() -> Result<Vec<PathBuf>, String> {
        let mut name = vec![0u16; VOLUME_NAME_LEN];
        // SAFETY: `name` holds `name.len()` writable units.
        let find = unsafe { FindFirstVolumeW(name.as_mut_ptr(), name.len() as u32) };
        if find == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().to_string());
        }
        let mut out = Vec::new();
        let result = loop {
            if let Err(e) = paths_of(&name, &mut out) {
                break Err(e);
            }
            // SAFETY: `find` is the open search; `name` holds `name.len()` writable units.
            if unsafe { FindNextVolumeW(find, name.as_mut_ptr(), name.len() as u32) } == 0 {
                let e = io::Error::last_os_error();
                break if e.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    Ok(out)
                } else {
                    Err(e.to_string())
                };
            }
        };
        // SAFETY: `find` came from FindFirstVolumeW and is closed once.
        unsafe { FindVolumeClose(find) };
        result
    }

    /// Adds each path the volume `name` (NUL-terminated) is mounted at to `out`.
    fn paths_of(name: &[u16], out: &mut Vec<PathBuf>) -> Result<(), String> {
        let mut buf = vec![0u16; 1024];
        loop {
            let mut needed = 0u32;
            // SAFETY: `name` is NUL-terminated; `buf` holds `buf.len()` writable units; `needed` is a valid u32.
            let ok = unsafe {
                GetVolumePathNamesForVolumeNameW(
                    name.as_ptr(),
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut needed,
                )
            };
            if ok != 0 {
                break;
            }
            let e = io::Error::last_os_error();
            if e.raw_os_error() == Some(ERROR_MORE_DATA as i32) && needed as usize > buf.len() {
                buf = vec![0u16; needed as usize];
                continue;
            }
            let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            return Err(format!(
                "Windows wouldn't say where {} is mounted: {e}",
                OsString::from_wide(&name[..end]).to_string_lossy()
            ));
        }
        // A list of NUL-terminated paths, ended by an empty one.
        for path in buf.split(|&c| c == 0).take_while(|p| !p.is_empty()) {
            let path = OsString::from_wide(path);
            let Some(text) = path.to_str() else {
                return Err(format!(
                    "the mount point {} isn't Unicode",
                    path.to_string_lossy()
                ));
            };
            out.push(PathBuf::from(text));
        }
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use std::path::PathBuf;

    pub(super) fn list() -> Result<Vec<PathBuf>, String> {
        Err("Vacancy reads the list of mounts on Windows and macOS only".into())
    }
}

#[cfg(test)]
mod tests {
    /// The list holds the system's own drive: `C:\` (or wherever Windows is), or `/`.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn lists_the_systems_own_mount() {
        let mounts = super::list().expect("the OS lists its mounts");
        let system = crate::volume::system_volume().expect("the system drive");
        let want = system.mount_point.to_lowercase();
        assert!(
            mounts
                .iter()
                .any(|m| m.to_string_lossy().to_lowercase() == want),
            "{want} in {mounts:?}"
        );
    }
}
