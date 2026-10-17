//! Windows installed-proof helper. Holds the fixture's existing mutation target and its real folder chain before
//! the scan, prints READY, then appends through that same handle on one `change` line on stdin. Never creates a file.
//! Build with `cargo build -p vacancy-core --example fixture_mutator`; tools/fixture-proof.mjs drives its protocol.

#[cfg(windows)]
mod windows {
    use std::ffi::OsStr;
    use std::fs::{File, OpenOptions};
    use std::io::{self, BufRead, Seek, SeekFrom, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::{Component, Path, PathBuf, Prefix};
    use std::ptr;
    use vacancy_core::fixture;
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::NtOpenFile;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
        FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    const TARGET: &str = "Users/fixture/AppData/Local/Google/Chrome/User Data/Profile 1/Cache/Cache_Data/modified-later.bin";
    const CHANGE: &[u8] = b"written after the scan\n";

    fn refused(reason: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, reason)
    }

    // Same parent-handle-relative, no-follow NT open pattern as the core executor. A directory can acquire a
    // reparse tag through an attribute-only handle despite share restrictions; resolving an absolute child path
    // again would then follow it. RootDirectory anchors lookup to the already opened directory object instead.
    fn open_in(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
        let wide: Vec<u16> = name.encode_wide().collect();
        if wide.is_empty() || wide.contains(&u16::from(b'\\')) || wide.contains(&u16::from(b'/')) {
            return Err(refused("expected one child name"));
        }
        let length = u16::try_from(wide.len() * 2).map_err(|_| refused("child name too long"))?;
        let object_name = UNICODE_STRING {
            Length: length,
            MaximumLength: length,
            Buffer: wide.as_ptr().cast_mut(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &object_name,
            Attributes: 0,
            SecurityDescriptor: ptr::null(),
            SecurityQualityOfService: ptr::null(),
        };
        let mut status_block = IO_STATUS_BLOCK::default();
        let mut handle: HANDLE = ptr::null_mut();
        // Synchronous, backup intent, no-follow, and directory/non-directory respectively.
        let options = 0x20 | 0x4000 | 0x0020_0000 | if directory { 0x1 } else { 0x40 };
        let access = SYNCHRONIZE
            | if directory {
                FILE_READ_ATTRIBUTES
            } else {
                GENERIC_READ | GENERIC_WRITE
            };
        // SAFETY: every pointer borrows live data for this call; parent is a live directory handle.
        let status = unsafe {
            NtOpenFile(
                &mut handle,
                access,
                &attributes,
                &mut status_block,
                if directory { FILE_SHARE_READ } else { 0 },
                options,
            )
        };
        if status < 0 || handle.is_null() {
            return Err(io::Error::other(format!(
                "relative no-follow open failed: NTSTATUS {status:#x}"
            )));
        }
        // SAFETY: NtOpenFile returned a new owned handle on success; File now closes it exactly once.
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    fn real_directory(file: &File) -> io::Result<()> {
        let meta = file.metadata()?;
        if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(refused("fixture ancestor must be a real directory"));
        }
        Ok(())
    }

    fn identity(file: &File) -> io::Result<(u64, [u8; 16])> {
        let mut id = FILE_ID_INFO::default();
        // SAFETY: file owns a live handle; id is writable for exactly its passed size.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut id as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok((id.VolumeSerialNumber, id.FileId.Identifier))
    }

    fn single_file(file: &File) -> io::Result<()> {
        let meta = file.metadata()?;
        if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(refused(
                "mutation target must be a regular file, never a reparse point",
            ));
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file owns a live handle; info is writable for the duration of the call.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.nNumberOfLinks != 1 {
            return Err(refused("mutation target must have exactly one name"));
        }
        Ok(())
    }

    struct Mutation {
        target: PathBuf,
        file: File,
        id: (u64, [u8; 16]),
        // Every ancestor stays open without delete sharing until the write is complete.
        _folders: Vec<File>,
    }

    impl Mutation {
        fn prepare(root: &Path, home: &Path) -> io::Result<Self> {
            Self::prepare_with(root, home, |_| {})
        }

        fn prepare_with(
            root: &Path,
            home: &Path,
            mut after_directory: impl FnMut(&Path),
        ) -> io::Result<Self> {
            if !root.is_absolute()
                || root
                    .components()
                    .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
            {
                return Err(refused(
                    "fixture root must be absolute without dot components",
                ));
            }
            let target = root.join(TARGET);
            let mut components = target.components();
            let letter = match components.next() {
                Some(Component::Prefix(p)) => match p.kind() {
                    Prefix::Disk(c) | Prefix::VerbatimDisk(c) => c,
                    _ => return Err(refused("only local drive paths are supported")),
                },
                _ => return Err(refused("local drive root required")),
            };
            if components.next() != Some(Component::RootDir) {
                return Err(refused("absolute drive path required"));
            }
            let mut path = PathBuf::from(format!(r"\\?\{}:\", char::from(letter)));
            // Only the immutable drive root is opened by absolute path. Every descendant is one name relative to
            // the previous handle, so even an in-place junction retag cannot redirect lookup to an outside file.
            let root_dir = OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)?;
            real_directory(&root_dir)?;
            let volume = identity(&root_dir)?.0;
            let mut folders = vec![root_dir];
            let names = components
                .map(|p| match p {
                    Component::Normal(n) => Ok(n),
                    _ => Err(refused("expected plain path components")),
                })
                .collect::<io::Result<Vec<_>>>()?;
            for name in &names[..names.len() - 1] {
                let dir = open_in(folders.last().unwrap(), name, true)?;
                real_directory(&dir)?;
                let serial = identity(&dir)?.0;
                if volume != serial {
                    return Err(refused("fixture chain crosses volumes"));
                }
                path.push(name);
                folders.push(dir);
                after_directory(&path);
            }
            fixture::check_root(root, home).map_err(io::Error::other)?;
            if !fixture::is_profile(root) {
                return Err(refused("generated fixture marker required"));
            }
            // No create/truncate and no sharing. The scanner lists this ordinary allocated fixture file from its
            // parent. Until released, no replacement or rename may redirect its later write. Windows still allows
            // a new hard link, so change checks the link count again before any write.
            let file = open_in(folders.last().unwrap(), names.last().unwrap(), false)?;
            single_file(&file)?;
            let id = identity(&file)?;
            if id.0 != volume {
                return Err(refused("mutation target is on another volume"));
            }
            Ok(Self {
                target,
                file,
                id,
                _folders: folders,
            })
        }

        fn change(mut self) -> io::Result<()> {
            for folder in &self._folders {
                real_directory(folder)?;
            }
            single_file(&self.file)?;
            // This is not an atomic no-hard-links guarantee: a process can add a name after this check, and that
            // alias would observe this same fixture inode's append. The held identity cannot become another file.
            if identity(&self.file)? != self.id {
                return Err(refused("mutation target identity changed"));
            }
            self.file.seek(SeekFrom::End(0))?;
            self.file.write_all(CHANGE)?;
            self.file.sync_all()?;
            // self drops the exclusive target and pinned folders before the caller announces completion.
            Ok(())
        }
    }

    pub fn run() -> io::Result<()> {
        let root = std::env::args_os()
            .nth(1)
            .map(PathBuf::from)
            .ok_or_else(|| refused("fixture root required"))?;
        let home = std::env::home_dir().ok_or_else(|| refused("real home is unknown"))?;
        let armed = Mutation::prepare(&root, &home)?;
        println!(
            "{}",
            serde_json::json!({ "state": "READY", "path": armed.target, "volume": armed.id.0.to_string(), "fileId": armed.id.1 })
        );
        io::stdout().flush()?;
        let command = io::stdin().lock().lines().next().transpose()?;
        if command.as_deref() != Some("change") {
            return Err(refused("expected exactly one change command"));
        }
        armed.change()?;
        println!("CHANGED");
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;
        use std::process::Command;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
        };
        use windows_sys::Win32::System::IO::DeviceIoControl;
        use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

        fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("fixture");
            let home = temp.path().join("separate-home");
            let file = root.join(TARGET);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::create_dir(&home).unwrap();
            fs::write(root.join(fixture::MARKER), b"fixture").unwrap();
            fs::write(&file, b"original").unwrap();
            (temp, root, home, file)
        }

        fn junction(link: &Path, to: &Path) {
            let out = Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(to)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
        }

        // Retag an existing empty directory without renaming it. Attribute-only access is deliberately used:
        // Windows sharing does not prevent these opens, even if FILE_SHARE_WRITE was omitted by a directory pin.
        fn retag_in_place(link: &Path, to: &Path) -> io::Result<()> {
            let file = OpenOptions::new()
                .access_mode(FILE_WRITE_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(link)?;
            let substitute = format!(r"\??\{}", to.display())
                .encode_utf16()
                .collect::<Vec<_>>();
            let print = to.as_os_str().encode_wide().collect::<Vec<_>>();
            let data_len = 8 + (substitute.len() + print.len() + 2) * 2;
            let mut data = Vec::new();
            data.extend_from_slice(&0xa000_0003u32.to_le_bytes());
            data.extend_from_slice(&(data_len as u16).to_le_bytes());
            data.extend_from_slice(&0u16.to_le_bytes());
            for n in [
                0,
                substitute.len() * 2,
                (substitute.len() + 1) * 2,
                print.len() * 2,
            ] {
                data.extend_from_slice(&(n as u16).to_le_bytes());
            }
            for n in substitute.into_iter().chain([0]).chain(print).chain([0]) {
                data.extend_from_slice(&n.to_le_bytes());
            }
            let mut returned = 0;
            // SAFETY: file is live; data holds a complete mount-point reparse buffer; returned is writable.
            let ok = unsafe {
                DeviceIoControl(
                    file.as_raw_handle(),
                    FSCTL_SET_REPARSE_POINT,
                    data.as_ptr().cast(),
                    data.len() as u32,
                    ptr::null_mut(),
                    0,
                    &mut returned,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

        #[test]
        fn an_ancestor_retagged_in_place_during_prepare_cannot_redirect_the_write() {
            let (temp, root, home, file) = setup();
            let cache_data = file.parent().unwrap();
            let cache = cache_data.parent().unwrap();
            let moved = temp.path().join("original-cache-data");
            let outside = temp.path().join("outside");
            fs::create_dir_all(outside.join("Cache_Data")).unwrap();
            let outside_file = outside.join("Cache_Data/modified-later.bin");
            fs::write(&outside_file, b"outside sentinel").unwrap();
            // Demonstrate a valid FSCTL control first, independently of the helper. Invalid setup cannot be
            // mistaken for successful protection.
            let control = temp.path().join("control");
            fs::create_dir(&control).unwrap();
            retag_in_place(&control, &outside).unwrap();
            assert_eq!(
                fs::read(control.join("Cache_Data/modified-later.bin")).unwrap(),
                b"outside sentinel"
            );
            fs::remove_dir(&control).unwrap();
            let mut retagged = false;
            let armed = Mutation::prepare_with(&root, &home, |path| {
                if path.file_name() == Some(OsStr::new("Cache")) {
                    fs::rename(cache_data, &moved).unwrap();
                    retag_in_place(cache, &outside).unwrap();
                    retagged = true;
                }
            });
            assert!(
                retagged,
                "the actual pinned ancestor was successfully retagged"
            );
            assert!(
                armed.is_err(),
                "relative child lookup must refuse rather than open the outside sentinel"
            );
            assert_eq!(
                fs::read(&file).unwrap(),
                b"outside sentinel",
                "an absolute reopen would now reach outside"
            );
            assert_eq!(fs::read(&outside_file).unwrap(), b"outside sentinel");
            assert_eq!(
                fs::read(moved.join("modified-later.bin")).unwrap(),
                b"original"
            );
            fs::remove_dir(cache).unwrap();
        }

        #[test]
        fn pinned_ancestors_block_a_junction_swap_and_only_the_original_changes() {
            let (temp, root, home, file) = setup();
            let outside = temp.path().join("outside");
            fs::create_dir(&outside).unwrap();
            let outside_file = outside.join("modified-later.bin");
            fs::write(&outside_file, b"outside").unwrap();
            let armed = Mutation::prepare(&root, &home).unwrap();
            let cache = file.parent().unwrap();
            assert!(
                fs::rename(cache, cache.with_extension("moved")).is_err(),
                "the pinned folder cannot move"
            );
            assert!(
                fs::rename(&root, root.with_extension("moved")).is_err(),
                "the pinned root cannot move"
            );
            assert!(
                fs::rename(&file, file.with_extension("moved")).is_err(),
                "the original file cannot be replaced"
            );
            armed.change().unwrap();
            assert_eq!(
                fs::read(&file).unwrap(),
                [b"original".as_slice(), CHANGE].concat()
            );
            assert_eq!(fs::read(&outside_file).unwrap(), b"outside");
            // After release, a swap really is possible. Preparing through it must now reject the junction and
            // leave the external target byte-identical. Remove only the junction entry before temp cleanup.
            let moved = cache.with_extension("moved");
            fs::rename(cache, &moved).unwrap();
            junction(cache, &outside);
            assert!(Mutation::prepare(&root, &home).is_err());
            assert_eq!(fs::read(&outside_file).unwrap(), b"outside");
            fs::remove_dir(cache).unwrap();
        }

        #[test]
        fn missing_or_hard_linked_targets_are_never_created_or_changed() {
            let (temp, root, home, file) = setup();
            let outside = temp.path().join("outside-file");
            fs::hard_link(&file, &outside).unwrap();
            assert!(Mutation::prepare(&root, &home).is_err());
            assert_eq!(fs::read(&outside).unwrap(), b"original");
            fs::remove_file(&file).unwrap();
            assert!(Mutation::prepare(&root, &home).is_err());
            assert!(!file.exists());
        }

        #[test]
        fn a_hard_link_added_while_waiting_refuses_the_change() {
            let (temp, root, home, file) = setup();
            let armed = Mutation::prepare(&root, &home).unwrap();
            let outside = temp.path().join("new-hardlink");
            fs::hard_link(&file, &outside).unwrap();
            assert!(armed.change().is_err());
            assert_eq!(fs::read(&file).unwrap(), b"original");
            assert_eq!(fs::read(&outside).unwrap(), b"original");
        }
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    match windows::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fixture mutation refused: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("fixture_mutator is Windows proof tooling only");
        std::process::ExitCode::FAILURE
    }
}
