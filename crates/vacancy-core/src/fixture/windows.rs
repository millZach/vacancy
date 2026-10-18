//! Native junction creation for the synthetic fixture generator only. No shell or deletion fallback.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::{mem, ptr};

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, UNICODE_STRING};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FileAttributeTagInfo,
    GetDriveTypeW, GetFileInformationByHandleEx, OPEN_EXISTING, SYNCHRONIZE,
};
use windows_sys::Win32::System::IO::{DeviceIoControl, IO_STATUS_BLOCK};
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
use windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOTE;

const TAG_MOUNT_POINT: u32 = 0xa000_0003;
const MAX_REPARSE_BYTES: usize = 16 * 1024;
const FILE_OPEN: u32 = 1;
const FILE_CREATE: u32 = 2;
const FILE_CREATED: usize = 2;
const FILE_DIRECTORY_FILE: u32 = 1;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x20;
const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

struct LocalPath {
    drive: u8,
    names: Vec<OsString>,
    ordinary: PathBuf,
}

fn local_path(path: &Path) -> io::Result<LocalPath> {
    if path.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err(invalid("fixture junction path contains .."));
    }
    let absolute = std::path::absolute(path)?;
    let mut parts = absolute.components();
    let drive = match parts.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) if drive.is_ascii_alphabetic() => drive,
            _ => return Err(invalid("fixture junction requires a local drive path")),
        },
        _ => return Err(invalid("fixture junction path lacks a drive")),
    };
    if parts.next() != Some(Component::RootDir) {
        return Err(invalid("fixture junction path is not absolute"));
    }
    let mut names = Vec::new();
    let mut ordinary = PathBuf::from(format!("{}:\\", char::from(drive)));
    for part in parts {
        let Component::Normal(name) = part else {
            return Err(invalid("fixture junction requires ordinary components"));
        };
        let units: Vec<u16> = name.encode_wide().take(256).collect();
        if units.is_empty() || units.len() > 255
            || units.iter().any(|u| *u < 32 || matches!(*u, 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124))
            || matches!(units.last(), Some(32 | 46))
        {
            return Err(invalid("invalid fixture junction path component"));
        }
        ordinary.push(name);
        names.push(name.to_owned());
        if names.len() > 512 {
            return Err(invalid("fixture junction component bound exceeded"));
        }
    }
    if names.is_empty() {
        return Err(invalid("fixture junction cannot name a drive root"));
    }
    Ok(LocalPath { drive, names, ordinary })
}

/// REPARSE_DATA_BUFFER: 8-byte header, 8-byte mount offsets, then two NUL-terminated UTF-16 names.
fn mount_buffer(target: &LocalPath) -> io::Result<Vec<u8>> {
    let print: Vec<u16> = target.ordinary.as_os_str().encode_wide().take(MAX_REPARSE_BYTES).collect();
    let substitute: Vec<u16> = OsStr::new(r"\??\").encode_wide().chain(print.iter().copied()).collect();
    let sub_bytes = substitute.len().checked_mul(2).ok_or_else(|| invalid("junction size overflow"))?;
    let print_bytes = print.len().checked_mul(2).ok_or_else(|| invalid("junction size overflow"))?;
    let data_bytes = 8usize.checked_add(sub_bytes).and_then(|n| n.checked_add(print_bytes))
        .and_then(|n| n.checked_add(4)).ok_or_else(|| invalid("junction size overflow"))?;
    if data_bytes + 8 > MAX_REPARSE_BYTES {
        return Err(invalid("fixture junction target exceeds reparse buffer bound"));
    }
    let sub_len = u16::try_from(sub_bytes).map_err(|_| invalid("junction substitute length"))?;
    let print_len = u16::try_from(print_bytes).map_err(|_| invalid("junction print length"))?;
    let print_offset = sub_len.checked_add(2).ok_or_else(|| invalid("junction print offset"))?;
    let data_len = u16::try_from(data_bytes).map_err(|_| invalid("junction data length"))?;
    let mut buffer = Vec::with_capacity(data_bytes + 8);
    buffer.extend_from_slice(&TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend_from_slice(&data_len.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    for word in [0, sub_len, print_offset, print_len] {
        buffer.extend_from_slice(&word.to_le_bytes());
    }
    for word in substitute.into_iter().chain([0]).chain(print).chain([0]) {
        buffer.extend_from_slice(&word.to_le_bytes());
    }
    Ok(buffer)
}

fn ordinary_directory(file: &File) -> io::Result<()> {
    // SAFETY: FILE_ATTRIBUTE_TAG_INFO is plain writable output storage.
    let mut tag: FILE_ATTRIBUTE_TAG_INFO = unsafe { mem::zeroed() };
    // SAFETY: the handle is owned/live and the output storage has the advertised type and size.
    let ok = unsafe { GetFileInformationByHandleEx(file.as_raw_handle(), FileAttributeTagInfo,
        (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(), mem::size_of_val(&tag) as u32) };
    if ok == 0 { return Err(io::Error::last_os_error()); }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 || tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(invalid("fixture junction ancestor/target is a link or not a directory"));
    }
    Ok(())
}

fn child(parent: &File, name: &OsStr, create: bool) -> io::Result<File> {
    let units: Vec<u16> = name.encode_wide().collect();
    let length = u16::try_from(units.len() * 2).map_err(|_| invalid("junction child name bound"))?;
    let name = UNICODE_STRING { Length:length, MaximumLength:length, Buffer:units.as_ptr().cast_mut() };
    let attributes = OBJECT_ATTRIBUTES { Length:mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory:parent.as_raw_handle(), ObjectName:&name, Attributes:0,
        SecurityDescriptor:ptr::null(), SecurityQualityOfService:ptr::null() };
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: IO_STATUS_BLOCK is plain writable output storage.
    let mut result: IO_STATUS_BLOCK = unsafe { mem::zeroed() };
    let access = FILE_READ_ATTRIBUTES | SYNCHRONIZE | if create { FILE_GENERIC_WRITE } else { 0 };
    // SAFETY: live retained parent, exact single-component UTF-16 name, correctly sized output pointers.
    // FILE_CREATE atomically fails on existing entries; no share mode grants access to a newly created child.
    let status = unsafe { NtCreateFile(&mut raw, access, &attributes, &mut result, ptr::null(),
        FILE_ATTRIBUTE_NORMAL, if create {0} else {FILE_SHARE_READ | FILE_SHARE_WRITE},
        if create {FILE_CREATE} else {FILE_OPEN},
        FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
        ptr::null(), 0) };
    if status < 0 {
        return Err(io::Error::other(format!("fixture junction native {} failed: NTSTATUS {:#010x}",
            if create {"create"} else {"open"}, status as u32)));
    }
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::other("native junction operation returned no handle"));
    }
    // SAFETY: successful NtCreateFile returned this uniquely owned handle; File closes it exactly once.
    let file = unsafe { File::from_raw_handle(raw) };
    if create && result.Information != FILE_CREATED {
        return Err(io::Error::other("junction creation did not report a new owned directory"));
    }
    ordinary_directory(&file)?;
    Ok(file)
}

/// Retains every ancestor without delete sharing; opens each next component relative to its retained parent.
fn directory_chain(path: &LocalPath, count: usize) -> io::Result<Vec<File>> {
    let root = format!(r"\\?\{}:\", char::from(path.drive));
    let wide: Vec<u16> = OsStr::new(&root).encode_wide().chain([0]).collect();
    // SAFETY: wide is NUL-terminated and valid for both native calls below.
    if unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_REMOTE {
        return Err(invalid("fixture junction requires a local volume"));
    }
    // SAFETY: valid terminated root string; null security attributes make this handle non-inheritable.
    let raw = unsafe { CreateFileW(wide.as_ptr(), FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE, ptr::null(), OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT, ptr::null_mut()) };
    if raw == INVALID_HANDLE_VALUE { return Err(io::Error::last_os_error()); }
    // SAFETY: CreateFileW returned a uniquely owned successful handle.
    let root = unsafe { File::from_raw_handle(raw) };
    ordinary_directory(&root)?;
    let mut chain = vec![root];
    for name in &path.names[..count] {
        let next = child(chain.last().ok_or_else(|| invalid("empty directory chain"))?,name,false)?;
        chain.push(next);
    }
    Ok(chain)
}

fn create_link(link: &LocalPath, buffer: &[u8]) -> io::Result<()> {
    let parents = directory_chain(link,link.names.len() - 1)?;
    let file = child(parents.last().ok_or_else(|| invalid("missing link parent"))?,
        link.names.last().ok_or_else(|| invalid("missing link name"))?,true)?;
    let mut returned = 0;
    // SAFETY: retained exclusively created no-follow handle; input is bounded readable bytes, no output/overlap.
    let ok = unsafe { DeviceIoControl(file.as_raw_handle(), FSCTL_SET_REPARSE_POINT,
        buffer.as_ptr().cast(), u32::try_from(buffer.len()).map_err(|_| invalid("junction ioctl length"))?,
        ptr::null_mut(), 0, &mut returned, ptr::null_mut()) };
    if ok == 0 {
        // Preserve the newly created directory. Never delete/reopen by path or substitute another link type.
        return Err(io::Error::other(format!("setting fixture junction {} failed; created directory retained: {}",
            link.ordinary.display(), io::Error::last_os_error())));
    }
    Ok(())
}

pub(super) fn junction(link: &Path, target: &Path) -> io::Result<()> {
    let link = local_path(link)?;
    let target = local_path(target)?;
    // Validate/encode before any creation. Target ancestors are no-follow and retained until ioctl completes.
    let buffer = mount_buffer(&target)?;
    let _target_handles = directory_chain(&target,target.names.len())?;
    create_link(&link,&buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn isolated() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("isolated temporary directory");
        let root = fs::canonicalize(directory.path()).expect("resolved test directory");
        (directory,root)
    }

    #[test]
    fn junction_names_are_literal_and_targets_untouched() {
        let (_directory,root) = isolated();
        let target = root.join("target space Résumé & % ! ^ (literal)");
        let link = root.join("link space 雪 & % ! ^ (literal)");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("sentinel.txt"),b"must survive unchanged").unwrap();
        super::super::link_dir(&link,&target).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::canonicalize(&link).unwrap(),fs::canonicalize(&target).unwrap());
        assert_eq!(fs::read(link.join("sentinel.txt")).unwrap(),b"must survive unchanged");
        assert_eq!(fs::read(target.join("sentinel.txt")).unwrap(),b"must survive unchanged");
        assert_eq!(fs::read_dir(&root).unwrap().count(),2);
        assert_eq!(fs::read_dir(&target).unwrap().count(),1);
    }

    #[test]
    fn existing_entries_are_never_repurposed() {
        let (_directory,root) = isolated();
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("sentinel"),b"target sentinel").unwrap();
        for (name,is_file) in [("existing-file",true),("existing-empty",false),("existing-full",false)] {
            let link = root.join(name);
            if is_file { fs::write(&link,b"existing file").unwrap(); }
            else { fs::create_dir(&link).unwrap(); }
            if name == "existing-full" { fs::write(link.join("sentinel"),b"existing sentinel").unwrap(); }
            assert!(junction(&link,&target).is_err());
            assert!(!fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            if is_file { assert_eq!(fs::read(&link).unwrap(),b"existing file"); }
            else { assert_eq!(fs::read_dir(&link).unwrap().count(),usize::from(name == "existing-full")); }
        }
        let existing = root.join("existing-junction");
        junction(&existing,&target).unwrap();
        let other = root.join("other");
        fs::create_dir(&other).unwrap();
        assert!(junction(&existing,&other).is_err());
        assert_eq!(fs::canonicalize(&existing).unwrap(),fs::canonicalize(&target).unwrap());
        assert_eq!(fs::read(target.join("sentinel")).unwrap(),b"target sentinel");
        assert_eq!(fs::read_dir(&other).unwrap().count(),0);
    }

    #[test]
    fn linked_ancestors_and_missing_targets_fail_before_creation() {
        let (_directory,root) = isolated();
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("sentinel"),b"untouched").unwrap();
        let alias = root.join("alias");
        junction(&alias,&target).unwrap();
        assert!(junction(&alias.join("must-not-exist"),&target).is_err());
        assert!(!target.join("must-not-exist").exists());
        let link = root.join("new-link");
        assert!(junction(&link,&alias).is_err());
        assert!(!link.exists());
        assert!(junction(&link,&root.join("missing-target")).is_err());
        assert!(!link.exists());
        assert_eq!(fs::read(target.join("sentinel")).unwrap(),b"untouched");
    }

    #[test]
    fn ioctl_failure_preserves_owned_empty_directory_without_fallback() {
        let (_directory,root) = isolated();
        let link = root.join("failed-junction");
        let error = create_link(&local_path(&link).unwrap(),&[]).unwrap_err();
        assert!(error.to_string().contains("created directory retained"));
        let metadata = fs::symlink_metadata(&link).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::read_dir(&link).unwrap().count(),0);
        assert_eq!(fs::read_dir(&root).unwrap().count(),1);
    }

    #[test]
    fn reparse_buffer_bounds_and_native_prefix_are_checked_without_io() {
        let local = local_path(Path::new(r"C:\fixture\Résumé & literal%")).unwrap();
        let buffer = mount_buffer(&local).unwrap();
        let verbatim = local_path(Path::new(r"\\?\C:\fixture\Résumé & literal%")).unwrap();
        assert_eq!(mount_buffer(&verbatim).unwrap(),buffer);
        assert_eq!(u32::from_le_bytes(buffer[..4].try_into().unwrap()),TAG_MOUNT_POINT);
        assert_eq!(usize::from(u16::from_le_bytes(buffer[4..6].try_into().unwrap())) + 8,buffer.len());
        let substitute_len = usize::from(u16::from_le_bytes(buffer[10..12].try_into().unwrap()));
        let print_offset = usize::from(u16::from_le_bytes(buffer[12..14].try_into().unwrap()));
        assert_eq!(print_offset,substitute_len + 2);
        let units = buffer[16..16 + substitute_len].chunks_exact(2)
            .map(|word| u16::from_le_bytes([word[0],word[1]])).collect::<Vec<_>>();
        assert_eq!(String::from_utf16(&units).unwrap(),r"\??\C:\fixture\Résumé & literal%");
        let long = PathBuf::from(format!("C:\\{}",vec!["a".repeat(240);40].join("\\")));
        assert!(mount_buffer(&local_path(&long).unwrap()).is_err());
        assert!(local_path(Path::new(r"\\server\share\target")).is_err());
        assert!(local_path(Path::new(r"C:\fixture\..\target")).is_err());
    }
}
