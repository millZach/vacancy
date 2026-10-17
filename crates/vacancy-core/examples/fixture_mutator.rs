//! Windows installed-proof helper. Holds the fixture's existing mutation target and its real folder chain before
//! the scan, prints READY, then appends through that same handle on one `change` line on stdin. Never creates a file.
//! Build with `cargo build -p vacancy-core --example fixture_mutator`; tools/fixture-proof.mjs drives its protocol.

#[cfg(windows)]
mod windows {
    use std::fs::{File, OpenOptions};
    use std::io::{self, BufRead, Seek, SeekFrom, Write};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use std::path::{Component, Path, PathBuf};
    use vacancy_core::fixture;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    };

    const TARGET: &str = "Users/fixture/AppData/Local/Google/Chrome/User Data/Profile 1/Cache/Cache_Data/modified-later.bin";
    const CHANGE: &[u8] = b"written after the scan\n";

    fn refused(reason: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, reason)
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
            let mut folders = Vec::new();
            let mut volume = None;
            // Pin top down. Each absolute open resolves only through already pinned real ancestors. Opening the
            // next entry as a reparse point and checking its handle closes the check/open substitution window.
            for path in target
                .parent()
                .unwrap()
                .ancestors()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                let dir = OpenOptions::new()
                    .access_mode(FILE_READ_ATTRIBUTES)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                    .open(path)?;
                let meta = dir.metadata()?;
                if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(refused("fixture ancestor must be a real directory"));
                }
                let serial = identity(&dir)?.0;
                if volume.is_some_and(|v| v != serial) {
                    return Err(refused("fixture chain crosses volumes"));
                }
                volume = Some(serial);
                folders.push(dir);
            }
            fixture::check_root(root, home).map_err(io::Error::other)?;
            if !fixture::is_profile(root) {
                return Err(refused("generated fixture marker required"));
            }
            // No create/truncate and no sharing. The scanner lists this ordinary allocated fixture file from its
            // parent. Until released, no replacement or rename may redirect its later write. Windows still allows
            // a new hard link, so change checks the link count again before any write.
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&target)?;
            single_file(&file)?;
            let id = identity(&file)?;
            if Some(id.0) != volume {
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
            single_file(&self.file)?;
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
