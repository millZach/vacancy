//! Private Windows move/journal capabilities, reusing the executor's parent-relative opens and held ancestry.
//! No absolute child reopen, replacement rename, copy fallback, or recursive removal exists here.
use super::*;
use crate::reorg::names::Name;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_OPEN, FILE_OPEN_IF, FileRenameInformation, NtCreateFile, NtSetInformationFile,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_WRITE, FILE_READ_DATA, FILE_RENAME_INFO,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Id {
    pub(crate) volume: u64,
    #[serde(with = "decimal128")]
    pub(crate) file: u128,
}

impl Id {
    pub(crate) fn step(step: &Step) -> Self {
        Self {
            volume: step.id.volume(),
            file: step.id.scan_key(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fingerprint {
    pub(crate) id: Id,
    pub(crate) logical: u64,
    pub(crate) allocated: u64,
    #[serde(with = "signed_decimal128")]
    pub(crate) changed: i128,
    pub(crate) links: u32,
}

mod signed_decimal128 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &i128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i128, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

pub(crate) fn nonce() -> Result<String, Fail> {
    use windows_sys::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    let mut bytes = [0u8; 16];
    // SAFETY: system RNG with its required null provider and a live fixed-size output buffer.
    let status = unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(Fail::Unsupported);
    }
    Ok(bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(""))
}

mod decimal128 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

pub(crate) struct PinnedFile(Handle);

impl std::fmt::Debug for PinnedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedFile").finish_non_exhaustive()
    }
}

pub(crate) fn directory(path: &Path) -> Result<Parent, Fail> {
    if !plain(path) {
        return Err(Fail::Unsupported);
    }
    // The synthetic final component is never opened. walk_to_parent retains the actual directory and ancestors.
    let parent = walk_to_parent(&path.join("vacancy-not-opened"), true)?;
    for dir in parent._above.iter().chain([&parent.dir]) {
        if attribute_tag(&dir.handle).ok_or(Fail::Unsupported)?.0 & FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err(Fail::Link);
        }
    }
    Ok(parent)
}

pub(crate) fn child_dir(parent: &Dir, name: &Name) -> Result<(Dir, Step), Fail> {
    let (dir, step) = open_dir(parent, &name.os(), None, Open::Pin)?;
    if attribute_tag(&dir.handle).ok_or(Fail::Unsupported)?.0 & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Fail::Link);
    }
    Ok((dir, step))
}

pub(crate) fn exists(parent: &Dir, name: &Name) -> Result<bool, Fail> {
    match nt_open_flags(
        &parent.handle,
        &name.0,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        0,
        0x40,
    ) {
        Ok(_) => Ok(true),
        Err(STATUS_OBJECT_NAME_NOT_FOUND) => Ok(false),
        Err(error) => Err(open_fail(error)),
    }
}

/// A rejection-only reservation key. Native names are never rewritten or used through this key as authority.
/// Use Windows' single-unit uppercase mapping instead of lossy display strings or Unicode expansion.
pub(crate) fn case_key(name: &Name) -> Vec<u16> {
    name.0
        .iter()
        .map(|&unit| {
            // SAFETY: pure scalar mapping, no pointer/handle or allocation contract.
            unsafe { windows_sys::Wdk::System::SystemServices::RtlUpcaseUnicodeChar(unit) }
        })
        .collect()
}

/// Metadata work is capped at the listing seam, including ignored entries, not just at the accepted-file count.
pub(crate) fn list_bounded(
    dir: &Dir,
    limit: usize,
    cancel: &crate::CancelToken,
) -> Result<(Vec<Entry>, bool), Fail> {
    let mut buf = vec![0u64; BUF_BYTES / 8];
    let mut out = Vec::new();
    let mut class = FileIdExtdDirectoryRestartInfo;
    loop {
        if cancel.is_cancelled() {
            return Ok((out, true));
        }
        // SAFETY: fixed aligned buffer, owned directory and matching buffer size.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.handle.0,
                class,
                buf.as_mut_ptr().cast(),
                BUF_BYTES as u32,
            )
        };
        class = FileIdExtdDirectoryInfo;
        if ok == 0 {
            // SAFETY: this thread's error from the immediately preceding native query.
            return match unsafe { GetLastError() } {
                ERROR_NO_MORE_FILES => Ok((out, false)),
                code => Err(set_fail(code)),
            };
        }
        let mut offset = 0;
        loop {
            if cancel.is_cancelled() || out.len() == limit {
                return Ok((out, true));
            }
            // SAFETY: same trusted native listing format/alignment as list(), one valid linked entry at offset.
            let (e, name, next) = unsafe { read_entry(&buf, offset) };
            if name != [46] && name != [46, 46] {
                out.push(to_entry(dir, &e, name));
            }
            if next == 0 {
                break;
            }
            offset += next as usize;
        }
    }
}

pub(crate) fn prepare_removal(
    parent: &Dir,
    name: &Name,
    expected: Id,
) -> Result<(Dir, Step), Fail> {
    let (dir, step) = open_dir(parent, &name.os(), None, Open::Delete)?;
    if Id::step(&step) != expected
        || attribute_tag(&dir.handle).ok_or(Fail::Unsupported)?.0 & FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    {
        return Err(Fail::Changed);
    }
    Ok((dir, step))
}

pub(crate) fn validate_directory(dir: &Dir, expected: Id) -> Result<(), Fail> {
    let (volume, file) = identity(&dir.handle).ok_or(Fail::Unsupported)?;
    if (Id { volume, file }) != expected
        || attribute_tag(&dir.handle).ok_or(Fail::Unsupported)?.0 & FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    {
        return Err(Fail::Changed);
    }
    Ok(())
}

pub(crate) fn remove_prepared(parent: &Dir, name: &Name, dir: Dir) -> Result<(), Fail> {
    let remove = 1u8;
    let mut io = IO_STATUS_BLOCK::default();
    // SAFETY: a live exclusive-delete directory handle and the one-byte native BOOLEAN disposition. The kernel
    // checks emptiness atomically; no recursion, attribute change, or file deletion fallback is used.
    let status = unsafe {
        NtSetInformationFile(
            dir.handle.0,
            &mut io,
            (&raw const remove).cast(),
            1,
            windows_sys::Wdk::Storage::FileSystem::FileDispositionInformation,
        )
    };
    if status < 0 {
        return Err(open_fail(status));
    }
    drop(dir);
    if exists(parent, name)? {
        Err(Fail::Changed)
    } else {
        Ok(())
    }
}

pub(crate) fn plain(path: &Path) -> bool {
    let mut parts = path.components();
    matches!(parts.next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)))
        && parts.next() == Some(Component::RootDir)
        && parts.all(|part| matches!(part, Component::Normal(n) if Name::checked(n.encode_wide().collect()).is_ok()))
}

/// Fresh identity/tag check on the retained objects, with no pathname reopen or link traversal.
pub(crate) fn revalidate(parent: &Parent) -> Result<(), Fail> {
    for (dir, step) in parent._above.iter().chain([&parent.dir]).zip(&parent.steps) {
        let (volume, file) = identity(&dir.handle).ok_or(Fail::Unsupported)?;
        if Identity::new(volume, file) != step.id
            || attribute_tag(&dir.handle).ok_or(Fail::Unsupported)?.0 & FILE_ATTRIBUTE_REPARSE_POINT
                != 0
        {
            return Err(Fail::Changed);
        }
    }
    Ok(())
}

pub(crate) fn file(
    parent: &Dir,
    name: &Name,
    exclusive: bool,
) -> Result<(PinnedFile, Fingerprint), Fail> {
    let handle = nt_open_flags(
        &parent.handle,
        &name.0,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE | if exclusive { DELETE } else { 0 },
        if exclusive {
            0
        } else {
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        },
        FILE_NON_DIRECTORY_FILE,
        if exclusive { 0x40 } else { 0 },
    )
    .map_err(open_fail)?;
    let pin = PinnedFile(handle);
    let info = fingerprint(&pin)?;
    Ok((pin, info))
}

pub(crate) fn fingerprint(file: &PinnedFile) -> Result<Fingerprint, Fail> {
    let (attributes, _) = attribute_tag(&file.0).ok_or(Fail::Unsupported)?;
    if attributes
        & (FILE_ATTRIBUTE_REPARSE_POINT
            | FILE_ATTRIBUTE_DIRECTORY
            | FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
    {
        return Err(Fail::Link);
    }
    let (volume, file_id) = identity(&file.0).ok_or(Fail::Unsupported)?;
    let b = basic(&file.0).ok_or(Fail::Unsupported)?;
    let s = standard(&file.0).ok_or(Fail::Unsupported)?;
    Ok(Fingerprint {
        id: Id {
            volume,
            file: file_id,
        },
        logical: u64::try_from(s.EndOfFile).map_err(|_| Fail::Unsupported)?,
        allocated: u64::try_from(s.AllocationSize).map_err(|_| Fail::Unsupported)?,
        changed: latest(&b),
        links: s.NumberOfLinks,
    })
}

pub(crate) fn rename(file: &PinnedFile, destination: &Dir, name: &Name) -> Result<(), Fail> {
    let (volume, _) = identity(&destination.handle).ok_or(Fail::Unsupported)?;
    if fingerprint(file)?.id.volume != volume {
        return Err(Fail::OtherVolume);
    }
    let length = mem::size_of::<FILE_RENAME_INFO>() + name.0.len() * 2;
    let mut storage = vec![0u64; length.div_ceil(8)];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: storage is aligned, zeroed, and sized for the header and the bounded native component. Zero flags
    // mean ReplaceIfExists=FALSE; the only destination is one component in the retained directory handle.
    unsafe {
        (*info).RootDirectory = destination.handle.0;
        (*info).FileNameLength = (name.0.len() * 2) as u32;
        ptr::copy_nonoverlapping(
            name.0.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast(),
            name.0.len(),
        );
    }
    let mut io = IO_STATUS_BLOCK::default();
    // SAFETY: source/destination owners outlive the synchronous call; all pointers refer to live sized buffers.
    let status = unsafe {
        NtSetInformationFile(
            file.0.0,
            &mut io,
            info.cast(),
            length as u32,
            FileRenameInformation,
        )
    };
    if status >= 0 {
        Ok(())
    } else {
        Err(open_fail(status))
    }
}

/// Create only, relative to a retained real parent. Returns the actual owned identity, never a fabricated one.
#[derive(Debug)]
pub(crate) enum MkdirFailure {
    Collision,
    Uncertain(Fail),
}

pub(crate) fn mkdir(parent: &Dir, name: &Name) -> Result<(Dir, Step), MkdirFailure> {
    let handle = create_status(
        &parent.handle,
        name,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        FILE_CREATE,
        FILE_DIRECTORY_FILE,
    )
    .map_err(|status| {
        if status == windows_sys::Win32::Foundation::STATUS_OBJECT_NAME_COLLISION {
            MkdirFailure::Collision
        } else {
            MkdirFailure::Uncertain(open_fail(status))
        }
    })?;
    let (attributes, _) =
        attribute_tag(&handle).ok_or(MkdirFailure::Uncertain(Fail::Unsupported))?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(MkdirFailure::Uncertain(Fail::Link));
    }
    let (volume, file) = identity(&handle).ok_or(MkdirFailure::Uncertain(Fail::Unsupported))?;
    Ok((
        Dir { handle },
        Step {
            name: name.os(),
            id: Identity::new(volume, file),
            mount: None,
        },
    ))
}

pub(crate) enum JournalOpen {
    Create,
    Existing,
}

pub(crate) fn journal_file(
    parent: &Dir,
    name: &Name,
    mode: JournalOpen,
) -> Result<(File, Id), Fail> {
    let handle = create(
        &parent.handle,
        name,
        // FlushFileBuffers documents GENERIC_WRITE; this is its mapped file access set.
        // The owning journal enforces exclusive append-at-EOF and exposes no overwrite operation.
        FILE_READ_DATA | FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES,
        0,
        match mode {
            JournalOpen::Create => FILE_CREATE,
            JournalOpen::Existing => FILE_OPEN,
        },
        FILE_NON_DIRECTORY_FILE,
    )?;
    journal_handle(handle)
}

/// Stable lease acquisition. Recovery cannot create or initialize anything; Begin may initialize only a
/// positively newly-created native file. A pre-existing empty marker is never adopted or overwritten.
pub(crate) fn lease_file(parent: &Dir, name: &Name, begin: bool) -> Result<(File, Id, bool), Fail> {
    use windows_sys::Win32::System::WindowsProgramming::{FILE_CREATED, FILE_OPENED};
    let access = FILE_READ_DATA
        | FILE_READ_ATTRIBUTES
        | if begin {
            FILE_GENERIC_WRITE
        } else {
            SYNCHRONIZE
        };
    let (handle, information) = create_receipt(
        &parent.handle,
        name,
        access,
        0,
        if begin { FILE_OPEN_IF } else { FILE_OPEN },
        FILE_NON_DIRECTORY_FILE,
    )
    .map_err(open_fail)?;
    let created = match information {
        value if begin && value == FILE_CREATED as usize => true,
        value if value == FILE_OPENED as usize => false,
        _ => return Err(Fail::Changed),
    };
    let (file, id) = journal_handle(handle)?;
    Ok((file, id, created))
}

fn journal_handle(handle: Handle) -> Result<(File, Id), Fail> {
    let (attributes, _) = attribute_tag(&handle).ok_or(Fail::Unsupported)?;
    if attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return Err(Fail::Link);
    }
    let (volume, file) = identity(&handle).ok_or(Fail::Unsupported)?;
    let s = standard(&handle).ok_or(Fail::Unsupported)?;
    if s.NumberOfLinks != 1 {
        return Err(Fail::Changed);
    }
    let raw = handle.0;
    mem::forget(handle);
    // SAFETY: this exclusively owned native file handle is transferred exactly once into File.
    Ok((unsafe { File::from_raw_handle(raw) }, Id { volume, file }))
}

pub(crate) fn journal_check(file: &File, expected: Id) -> Result<(), Fail> {
    // Borrow the owned File's handle without closing it. All native queries finish before File can be dropped.
    let borrowed = std::mem::ManuallyDrop::new(Handle(file.as_raw_handle()));
    let (volume, file) = identity(&borrowed).ok_or(Fail::Unsupported)?;
    if (Id { volume, file }) != expected
        || attribute_tag(&borrowed).ok_or(Fail::Unsupported)?.0
            & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY)
            != 0
        || standard(&borrowed).ok_or(Fail::Unsupported)?.NumberOfLinks != 1
    {
        return Err(Fail::Changed);
    }
    Ok(())
}

fn create(
    parent: &Handle,
    name: &Name,
    access: u32,
    share: u32,
    disposition: u32,
    options: u32,
) -> Result<Handle, Fail> {
    create_status(parent, name, access, share, disposition, options).map_err(open_fail)
}

fn create_status(
    parent: &Handle,
    name: &Name,
    access: u32,
    share: u32,
    disposition: u32,
    options: u32,
) -> Result<Handle, i32> {
    create_receipt(parent, name, access, share, disposition, options).map(|(handle, _)| handle)
}

fn create_receipt(
    parent: &Handle,
    name: &Name,
    access: u32,
    share: u32,
    disposition: u32,
    options: u32,
) -> Result<(Handle, usize), i32> {
    let object_name = UNICODE_STRING {
        Length: (name.0.len() * 2) as u16,
        MaximumLength: (name.0.len() * 2) as u16,
        Buffer: name.0.as_ptr().cast_mut(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.0,
        ObjectName: &object_name,
        Attributes: 0x40,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    let mut io = IO_STATUS_BLOCK::default();
    let mut raw = ptr::null_mut();
    // SAFETY: all pointers live through this synchronous call. CREATE never replaces; OPEN_IF is used only for
    // the stable journal lease/store. Every open is one validated component relative to its retained parent.
    let status = unsafe {
        NtCreateFile(
            &mut raw,
            access,
            &attributes,
            &mut io,
            ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            share,
            disposition,
            options
                | FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_FOR_BACKUP_INTENT
                | FILE_OPEN_REPARSE_POINT,
            ptr::null(),
            0,
        )
    };
    if status >= 0 && !raw.is_null() {
        Ok((Handle(raw), io.Information))
    } else {
        Err(status)
    }
}
