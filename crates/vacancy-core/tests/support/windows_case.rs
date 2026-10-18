//! Native case-sensitive directory setup shared by unit and integration tests.
//! Callers own fresh fixture directories and their unraced ancestors. This is not an adversarial path sandbox.

use std::path::Path;

/// Enables only the supplied fixture leaf and returns true only after observing its native flag.
pub(crate) fn make_case_sensitive(dir: &Path) -> bool {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
        FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES, FileAttributeTagInfo, FileCaseSensitiveInfo,
        GetFileInformationByHandleEx, SetFileInformationByHandle,
    };
    // FILE_CS_FLAG_CASE_SENSITIVE_DIR, defined by the Windows SDK for FILE_CASE_SENSITIVE_INFO.
    const CASE_SENSITIVE: u32 = 1;

    fn flags(file: &File) -> io::Result<u32> {
        let mut info = FILE_CASE_SENSITIVE_INFO::default();
        // SAFETY: the live handle and matching writable information buffer outlive the call.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileCaseSensitiveInfo,
                (&raw mut info).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(info.Flags)
    }
    let enable = || -> io::Result<()> {
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(dir)?;
        let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
        // SAFETY: the live handle and correctly sized writable native buffer outlive the query.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileAttributeTagInfo,
                (&raw mut tag).cast(),
                std::mem::size_of_val(&tag) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return Err(io::Error::other(
                "fixture leaf is not an ordinary directory",
            ));
        }
        let before = flags(&file)?;
        if before & CASE_SENSITIVE != 0 {
            eprintln!(
                "VERIFIED: native case-sensitive directory flag already enabled ({before:#x})"
            );
            return Ok(());
        }
        let info = FILE_CASE_SENSITIVE_INFO {
            Flags: before | CASE_SENSITIVE,
        };
        // SAFETY: this ordinary fixture handle has WRITE_ATTRIBUTES; the matching buffer is live.
        let set = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileCaseSensitiveInfo,
                (&raw const info).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        };
        let failure = if set == 0 {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        let after = flags(&file);
        if let Some(error) = failure {
            return Err(io::Error::other(format!(
                "native set failed: {error}; before={before:#x}; after={after:?}"
            )));
        }
        let after = after?;
        if after & CASE_SENSITIVE == 0 {
            return Err(io::Error::other(format!(
                "native set reported success but flag is absent: before={before:#x}; after={after:#x}"
            )));
        }
        eprintln!(
            "VERIFIED: native case-sensitive directory flag before={before:#x}, after={after:#x}"
        );
        Ok(())
    };
    match enable() {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "UNAVAILABLE: optional Windows case-sensitive fixture {}: {error}",
                dir.display()
            );
            false
        }
    }
}
