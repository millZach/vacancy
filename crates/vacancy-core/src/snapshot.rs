//! What a scan found: every folder on the volume, each file above a size threshold, and per-folder totals for the
//! rest. Memory grows with the number of folders, not files: a folder costs about a hundred bytes, a small file
//! nothing beyond its share of its folder's totals.
//!
//! The tree is an arena. A folder's subfolders sit next to each other in it, and so do its kept files, so a folder
//! only records where its runs start. Names are stored once, in one byte buffer, exactly as the OS spelled them.
//!
//! Totals are kept up to date while the scan runs: when a folder's listing arrives, its numbers are added to every
//! folder above it. So a snapshot lent out mid-scan (see [`crate::ScanProgress::preview`]) is consistent, just
//! smaller than the finished one.

use std::ffi::OsStr;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A folder in a [`Snapshot`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DirId(pub(crate) u32);

/// A kept file (one at or above the scan's size threshold) in a [`Snapshot`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub(crate) u32);

impl DirId {
    /// The folder's position in the snapshot, stable for the snapshot's life. Used to name it to the UI.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl FileId {
    /// The file's position in the snapshot, stable for the snapshot's life. Used to name it to the UI.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Seconds since the Unix epoch, UTC. Zero means unknown.
pub type UnixTime = i64;

/// Facts about a folder that the scanner noticed but did not count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DirFlags(u8);

impl DirFlags {
    /// The OS refused to list it (no permission, or macOS privacy protection). Its size is unknown.
    pub const UNREADABLE: Self = Self(1);
    /// A symlink, junction, mount point or other reparse point that names somewhere else. Never entered.
    pub const LINK: Self = Self(1 << 1);
    /// Another file system is mounted here. Never entered.
    pub const OTHER_VOLUME: Self = Self(1 << 2);
    /// The listing failed part way; what was read is counted.
    pub const INCOMPLETE: Self = Self(1 << 3);
    /// A cloud-sync placeholder folder (OneDrive, Dropbox). Entered: it is a real local folder, and only the bytes
    /// actually on this disk are counted.
    pub const CLOUD: Self = Self(1 << 4);
    /// When the scanner came to open it, its path led to a different folder than the one its parent listed: it,
    /// or a folder above it, was replaced or moved (perhaps by a link) during the scan. Never entered.
    pub const CHANGED: Self = Self(1 << 5);

    /// Whether every flag in `other` is set.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the scanner looked inside this folder.
    pub fn entered(self) -> bool {
        self.0 & (Self::UNREADABLE.0 | Self::LINK.0 | Self::OTHER_VOLUME.0 | Self::CHANGED.0) == 0
    }

    pub(crate) fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Files and folders the categorizer and the rules look for by name, noted per folder because small files are not
/// kept individually.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Markers(u16);

impl Markers {
    /// A `.git` folder or file: the folder is the root of a Git repository.
    pub const GIT: Self = Self(1);
    /// `package.json`: a Node project.
    pub const PACKAGE_JSON: Self = Self(1 << 1);
    /// `Cargo.toml`: a Rust project.
    pub const CARGO_TOML: Self = Self(1 << 2);
    /// `pyvenv.cfg`: the folder is a Python virtual environment.
    pub const PYVENV: Self = Self(1 << 3);
    /// `pyproject.toml`, `requirements.txt` or `setup.py`: a Python project.
    pub const PYTHON_PROJECT: Self = Self(1 << 4);
    /// `build.gradle`, `build.gradle.kts` or `settings.gradle`: a Gradle project.
    pub const GRADLE: Self = Self(1 << 5);
    /// A `*.xcodeproj` or `*.xcworkspace` folder: an Xcode project.
    pub const XCODE_PROJECT: Self = Self(1 << 6);
    /// A Node lockfile (`package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock`...): dependencies were
    /// installed here by a package manager, so this is a project, not an app shipping its own `node_modules`.
    pub const NODE_LOCK: Self = Self(1 << 7);
    /// `CACHEDIR.TAG` or `.rustc_info.json`: the folder is Cargo's build output.
    pub const CARGO_TARGET: Self = Self(1 << 8);
    /// `.tmp.drivedownload` or `.tmp.driveupload`: Google Drive for desktop's working folders, which it keeps at
    /// the top of every folder it mirrors, wherever the user put it.
    pub const GOOGLE_DRIVE_ROOT: Self = Self(1 << 9);
    /// `.dropbox` or `.dropbox.cache`: the top of a Dropbox folder.
    pub const DROPBOX_ROOT: Self = Self(1 << 10);
    /// A file named `Preferences`: Chromium and Electron keep one in every browser profile.
    pub const PREFERENCES: Self = Self(1 << 11);

    /// Whether every marker in `other` is present.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(crate) fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The marker a file of this name sets, if any.
    pub(crate) fn for_file(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            ".git" => Self::GIT, // a worktree or submodule has a .git file
            "package.json" => Self::PACKAGE_JSON,
            "cargo.toml" => Self::CARGO_TOML,
            "pyvenv.cfg" => Self::PYVENV,
            "pyproject.toml" | "requirements.txt" | "setup.py" => Self::PYTHON_PROJECT,
            "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts" => {
                Self::GRADLE
            }
            "package-lock.json"
            | "npm-shrinkwrap.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "bun.lock"
            | "bun.lockb" => Self::NODE_LOCK,
            "cachedir.tag" | ".rustc_info.json" => Self::CARGO_TARGET,
            ".dropbox" => Self::DROPBOX_ROOT,
            "preferences" => Self::PREFERENCES,
            _ => Self::default(),
        }
    }

    /// The marker a folder of this name sets in its parent, if any.
    pub(crate) fn for_dir(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower == ".git" {
            Self::GIT
        } else if lower.ends_with(".xcodeproj") || lower.ends_with(".xcworkspace") {
            Self::XCODE_PROJECT
        } else if lower == ".tmp.drivedownload" || lower == ".tmp.driveupload" {
            Self::GOOGLE_DRIVE_ROOT
        } else if lower == ".dropbox.cache" {
            Self::DROPBOX_ROOT
        } else {
            Self::default()
        }
    }

    /// Whether a name could set a marker. Cheap enough to call for every file.
    pub(crate) fn may_mark(name_len: usize) -> bool {
        (4..=20).contains(&name_len)
    }
}

/// Bytes, counts and the most recent use, for one folder's own files or for everything below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Allocated bytes: what the files take on disk, not their logical length.
    pub bytes: u64,
    /// Files, each hard-linked file counted once.
    pub files: u64,
    /// Folders below (zero for a folder's own totals).
    pub dirs: u64,
    /// The most recent of modified and accessed times, or zero when unknown.
    pub last_used: UnixTime,
}

/// A kept file: one at or above the scan's size threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInfo<'a> {
    /// The folder the file is in.
    pub parent: DirId,
    /// Its name, exactly as the OS spells it.
    pub name: &'a OsStr,
    /// Allocated bytes.
    pub bytes: u64,
    /// The most recent of modified and accessed times.
    pub last_used: UnixTime,
}

/// How a top-level folder of the snapshot relates to the volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootKind {
    /// The volume, or a folder on it (a test fixture).
    Volume,
    /// macOS's sealed, read-only system volume: macOS itself.
    SealedSystem,
    /// macOS's Data volume, shown at `/` the way Finder and firmlinks show it.
    Data,
}

/// Where a top-level folder of the snapshot is, and where it is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    /// The folder.
    pub dir: DirId,
    /// The path the scanner opened.
    pub real: PathBuf,
    /// The path people know it by. Usually `real`; macOS's Data volume (`/System/Volumes/Data`) is shown as `/`.
    pub shown: PathBuf,
    /// What it is.
    pub kind: RootKind,
}

/// The scan's own numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// Files counted, each hard-linked file once.
    pub files: u64,
    /// Folders found, including ones that could not be read.
    pub folders: u64,
    /// Allocated bytes counted.
    pub bytes: u64,
    /// Folders the OS refused to list.
    pub unreadable: u64,
    /// Links (symlinks, junctions, reparse points) seen and not followed.
    pub links_skipped: u64,
    /// Places where another file system is mounted, not entered.
    pub other_volumes_skipped: u64,
    /// Extra names of hard-linked files: seen again, not counted again.
    pub hard_links_merged: u64,
    /// Folders that were replaced or moved between being listed and being opened, and so were not entered.
    pub changed: u64,
    /// How long the scan took.
    pub elapsed: Duration,
}

#[derive(Clone, Copy, Debug)]
struct NameRef {
    start: u32,
    len: u32,
}

#[derive(Clone, Copy, Debug)]
struct Own {
    bytes: u64,
    files: u32,
    last_used: UnixTime,
}

#[derive(Clone, Copy, Debug)]
struct DirRec {
    parent: u32,
    name: NameRef,
    first_child: u32,
    child_count: u32,
    first_file: u32,
    file_count: u32,
    own: Own,
    tree: Totals,
    flags: DirFlags,
    markers: Markers,
}

#[derive(Clone, Copy, Debug)]
struct FileRec {
    parent: u32,
    name: NameRef,
    bytes: u64,
    last_used: UnixTime,
    /// What identified the file when it was listed (Windows file id; Unix device and inode), or 0 where the file
    /// system has none.
    identity: u128,
}

const NO_PARENT: u32 = u32::MAX;

/// The result of a scan. See the module docs.
#[derive(Debug)]
pub struct Snapshot {
    roots: Vec<Root>,
    dirs: Vec<DirRec>,
    /// Per folder, what identified it when it was listed (Windows file id; Unix device and inode), or 0 where the
    /// file system has none (FAT) and for the roots.
    ids: Vec<u128>,
    files: Vec<FileRec>,
    names: Vec<u8>,
    stats: ScanStats,
    keep_files_from: u64,
    /// When the scan began. Anything modified or changed since then is not what the scan saw.
    started: SystemTime,
}

impl Snapshot {
    /// The top-level folders: one for a volume, two for a macOS startup disk (system and Data volumes).
    pub fn roots(&self) -> &[Root] {
        &self.roots
    }

    /// The scan's numbers.
    pub fn stats(&self) -> ScanStats {
        self.stats
    }

    /// Files at or above this many bytes are kept individually; smaller ones only count in their folder's totals.
    pub fn keeps_files_from(&self) -> u64 {
        self.keep_files_from
    }

    /// How many folders the snapshot holds.
    pub fn dir_count(&self) -> usize {
        self.dirs.len()
    }

    /// How many files it keeps individually.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Every folder, parents before children.
    pub fn dirs(&self) -> impl ExactSizeIterator<Item = DirId> + DoubleEndedIterator + use<> {
        (0..self.dirs.len() as u32).map(DirId)
    }

    /// The folder with this index, if there is one.
    pub fn dir_at(&self, index: usize) -> Option<DirId> {
        (index < self.dirs.len()).then_some(DirId(index as u32))
    }

    /// The kept file with this index, if there is one.
    pub fn file_at(&self, index: usize) -> Option<FileId> {
        (index < self.files.len()).then_some(FileId(index as u32))
    }

    /// The folder above, or `None` for a root.
    pub fn parent(&self, dir: DirId) -> Option<DirId> {
        let p = self.dir(dir).parent;
        (p != NO_PARENT).then_some(DirId(p))
    }

    /// The folder's name. A root's name is its shown path.
    pub fn name(&self, dir: DirId) -> &OsStr {
        if let Some(root) = self.roots.iter().find(|r| r.dir == dir) {
            return root.shown.as_os_str();
        }
        self.name_of(self.dir(dir).name)
    }

    /// The folder's subfolders, including ones that were not entered (links, unreadable, other volumes).
    pub fn children(
        &self,
        dir: DirId,
    ) -> impl ExactSizeIterator<Item = DirId> + DoubleEndedIterator + use<> {
        let d = self.dir(dir);
        (d.first_child..d.first_child + d.child_count).map(DirId)
    }

    /// The folder's kept files.
    pub fn files(&self, dir: DirId) -> impl ExactSizeIterator<Item = FileId> + use<> {
        let d = self.dir(dir);
        (d.first_file..d.first_file + d.file_count).map(FileId)
    }

    /// Everything directly in the folder: all its files, kept or not. `dirs` is zero.
    pub fn own(&self, dir: DirId) -> Totals {
        let own = self.dir(dir).own;
        Totals {
            bytes: own.bytes,
            files: u64::from(own.files),
            dirs: 0,
            last_used: own.last_used,
        }
    }

    /// Everything in and below the folder.
    pub fn tree(&self, dir: DirId) -> Totals {
        self.dir(dir).tree
    }

    /// What the scanner noticed about the folder.
    pub fn flags(&self, dir: DirId) -> DirFlags {
        self.dir(dir).flags
    }

    /// What identified a folder when the scan listed it (Windows file id; Unix device and inode), if the file system
    /// has one. A folder found with another identity now is not the folder the scan saw.
    pub(crate) fn identity(&self, dir: DirId) -> Option<u128> {
        self.ids.get(dir.0 as usize).copied().filter(|&id| id != 0)
    }

    /// What identified a kept file when the scan listed it, as [`identity`](Self::identity) does for folders.
    pub(crate) fn file_identity(&self, file: FileId) -> Option<u128> {
        self.files
            .get(file.0 as usize)
            .map(|f| f.identity)
            .filter(|&id| id != 0)
    }

    /// When the scan began: anything in the snapshot that was modified or changed on disk since then is no longer
    /// what the scan saw.
    pub fn started(&self) -> SystemTime {
        self.started
    }

    /// Which marker files and folders it holds.
    pub fn markers(&self, dir: DirId) -> Markers {
        self.dir(dir).markers
    }

    /// A kept file.
    pub fn file(&self, file: FileId) -> FileInfo<'_> {
        let f = &self.files[file.0 as usize];
        FileInfo {
            parent: DirId(f.parent),
            name: self.name_of(f.name),
            bytes: f.bytes,
            last_used: f.last_used,
        }
    }

    /// The folder's shown path (see [`Root::shown`]).
    pub fn path(&self, dir: DirId) -> PathBuf {
        let mut names = Vec::new();
        let mut at = dir.0;
        loop {
            let d = &self.dirs[at as usize];
            if d.parent == NO_PARENT {
                let mut path = self.root_of(DirId(at)).shown.clone();
                for name in names.iter().rev() {
                    path.push(name);
                }
                return path;
            }
            names.push(self.name_of(d.name));
            at = d.parent;
        }
    }

    /// A kept file's shown path.
    pub fn file_path(&self, file: FileId) -> PathBuf {
        let f = self.file(file);
        self.path(f.parent).join(f.name)
    }

    /// The folder at `path` (a shown path), if the snapshot has it. Compares names exactly.
    pub fn find(&self, path: &Path) -> Option<DirId> {
        self.roots.iter().find_map(|root| {
            let rest = path.strip_prefix(&root.shown).ok()?;
            let mut at = root.dir;
            for part in rest.components() {
                let name = part.as_os_str();
                at = self
                    .children(at)
                    .find(|&c| self.name_of(self.dir(c).name) == name)?;
            }
            Some(at)
        })
    }

    /// The kept file at `path`, if the snapshot has it.
    pub fn find_file(&self, path: &Path) -> Option<FileId> {
        let dir = self.find(path.parent()?)?;
        let name = path.file_name()?;
        self.files(dir).find(|&f| self.file(f).name == name)
    }

    /// Folders the OS refused to list.
    pub fn unreadable(&self) -> impl Iterator<Item = DirId> + '_ {
        self.dirs()
            .filter(|&d| self.flags(d).contains(DirFlags::UNREADABLE))
    }

    /// The root a folder belongs to.
    pub fn root_of(&self, dir: DirId) -> &Root {
        let mut at = dir.0;
        while self.dirs[at as usize].parent != NO_PARENT {
            at = self.dirs[at as usize].parent;
        }
        self.roots
            .iter()
            .find(|r| r.dir.0 == at)
            .expect("every top-level folder is a root")
    }

    fn dir(&self, dir: DirId) -> &DirRec {
        &self.dirs[dir.0 as usize]
    }

    fn name_of(&self, name: NameRef) -> &OsStr {
        let bytes = &self.names[name.start as usize..(name.start + name.len) as usize];
        // SAFETY: every name was appended from `OsStr::as_encoded_bytes` on this platform, whole.
        unsafe { OsStr::from_encoded_bytes_unchecked(bytes) }
    }
}

/// A folder's listing, gathered by a scan worker without holding the snapshot's lock.
#[derive(Debug, Default)]
pub(crate) struct Listing {
    pub(crate) own_bytes: u64,
    pub(crate) own_files: u32,
    pub(crate) own_last_used: UnixTime,
    pub(crate) markers: Markers,
    pub(crate) flags: DirFlags,
    /// File-shaped links (symlinks) seen and not followed.
    pub(crate) file_links: u32,
    /// Extra names of hard-linked files, not counted again.
    pub(crate) hard_links: u32,
    pub(crate) subdirs: Vec<PendingDir>,
    pub(crate) kept: Vec<PendingFile>,
    names: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct PendingDir {
    name: Range<usize>,
    pub(crate) flags: DirFlags,
    pub(crate) modified: UnixTime,
    /// What identified the folder when it was listed (Windows file id; Unix device and inode), if known. Opening
    /// it later must find the same folder.
    pub(crate) identity: Option<u128>,
}

#[derive(Debug)]
pub(crate) struct PendingFile {
    name: Range<usize>,
    bytes: u64,
    last_used: UnixTime,
    identity: Option<u128>,
}

impl Listing {
    pub(crate) fn clear(&mut self) {
        self.own_bytes = 0;
        self.own_files = 0;
        self.own_last_used = 0;
        self.markers = Markers::default();
        self.flags = DirFlags::default();
        self.file_links = 0;
        self.hard_links = 0;
        self.subdirs.clear();
        self.kept.clear();
        self.names.clear();
    }

    pub(crate) fn push_dir(
        &mut self,
        name: &OsStr,
        flags: DirFlags,
        modified: UnixTime,
        identity: Option<u128>,
    ) {
        let name = self.push_name(name);
        self.subdirs.push(PendingDir {
            name,
            flags,
            modified,
            identity,
        });
    }

    pub(crate) fn push_kept(
        &mut self,
        name: &OsStr,
        bytes: u64,
        last_used: UnixTime,
        identity: Option<u128>,
    ) {
        let name = self.push_name(name);
        self.kept.push(PendingFile {
            name,
            bytes,
            last_used,
            identity,
        });
    }

    /// The name of a pending subfolder.
    pub(crate) fn dir_name(&self, dir: &PendingDir) -> &OsStr {
        // SAFETY: the bytes were appended from `OsStr::as_encoded_bytes`, whole.
        unsafe { OsStr::from_encoded_bytes_unchecked(&self.names[dir.name.clone()]) }
    }

    fn push_name(&mut self, name: &OsStr) -> Range<usize> {
        let start = self.names.len();
        self.names.extend_from_slice(name.as_encoded_bytes());
        start..self.names.len()
    }
}

/// Builds a snapshot while a scan runs. Lives behind the scan's lock.
#[derive(Debug)]
pub(crate) struct Builder {
    snap: Snapshot,
}

impl Builder {
    pub(crate) fn new(keep_files_from: u64, started: SystemTime) -> Self {
        Self {
            snap: Snapshot {
                roots: Vec::new(),
                dirs: Vec::new(),
                ids: Vec::new(),
                files: Vec::new(),
                names: Vec::new(),
                stats: ScanStats::default(),
                keep_files_from,
                started,
            },
        }
    }

    pub(crate) fn snapshot(&self) -> &Snapshot {
        &self.snap
    }

    pub(crate) fn add_root(&mut self, real: PathBuf, shown: PathBuf, kind: RootKind) -> DirId {
        let id = DirId(self.snap.dirs.len() as u32);
        self.snap.dirs.push(DirRec {
            parent: NO_PARENT,
            name: NameRef { start: 0, len: 0 },
            first_child: 0,
            child_count: 0,
            first_file: 0,
            file_count: 0,
            own: Own {
                bytes: 0,
                files: 0,
                last_used: 0,
            },
            tree: Totals::default(),
            flags: DirFlags::default(),
            markers: Markers::default(),
        });
        self.snap.ids.push(0);
        self.snap.roots.push(Root {
            dir: id,
            real,
            shown,
            kind,
        });
        self.snap.stats.folders += 1;
        id
    }

    /// Marks a folder that was not entered (unreadable, another volume).
    pub(crate) fn flag(&mut self, dir: DirId, flags: DirFlags) {
        let d = &mut self.snap.dirs[dir.0 as usize];
        d.flags = d.flags.with(flags);
        if flags.contains(DirFlags::UNREADABLE) {
            self.snap.stats.unreadable += 1;
        }
        if flags.contains(DirFlags::OTHER_VOLUME) {
            self.snap.stats.other_volumes_skipped += 1;
        }
        if flags.contains(DirFlags::CHANGED) {
            self.snap.stats.changed += 1;
        }
    }

    /// Records a folder's listing: its own files, its kept files and its subfolders, and adds its numbers to every
    /// folder above. Returns the new subfolders' ids, in the listing's order.
    pub(crate) fn commit(&mut self, dir: DirId, listing: &Listing) -> Range<u32> {
        let snap = &mut self.snap;
        let first_child = snap.dirs.len() as u32;
        let first_file = snap.files.len() as u32;
        let mut newest = listing.own_last_used;
        for sub in &listing.subdirs {
            let name = append_name(&mut snap.names, &listing.names[sub.name.clone()]);
            newest = newest.max(sub.modified);
            snap.dirs.push(DirRec {
                parent: dir.0,
                name,
                first_child: 0,
                child_count: 0,
                first_file: 0,
                file_count: 0,
                own: Own {
                    bytes: 0,
                    files: 0,
                    last_used: sub.modified,
                },
                tree: Totals {
                    bytes: 0,
                    files: 0,
                    dirs: 0,
                    last_used: sub.modified,
                },
                flags: sub.flags,
                markers: Markers::default(),
            });
            snap.ids.push(sub.identity.unwrap_or(0));
            if sub.flags.contains(DirFlags::LINK) {
                snap.stats.links_skipped += 1;
            }
            if sub.flags.contains(DirFlags::OTHER_VOLUME) {
                snap.stats.other_volumes_skipped += 1;
            }
        }
        for kept in &listing.kept {
            let name = append_name(&mut snap.names, &listing.names[kept.name.clone()]);
            snap.files.push(FileRec {
                parent: dir.0,
                name,
                bytes: kept.bytes,
                last_used: kept.last_used,
                identity: kept.identity.unwrap_or(0),
            });
        }
        let child_count = listing.subdirs.len() as u32;
        {
            let d = &mut snap.dirs[dir.0 as usize];
            d.first_child = first_child;
            d.child_count = child_count;
            d.first_file = first_file;
            d.file_count = listing.kept.len() as u32;
            d.own.bytes = listing.own_bytes;
            d.own.files = listing.own_files;
            d.own.last_used = d.own.last_used.max(listing.own_last_used);
            d.flags = d.flags.with(listing.flags);
            d.markers = listing.markers;
        }
        let mut at = dir.0;
        while at != NO_PARENT {
            let d = &mut snap.dirs[at as usize];
            d.tree.bytes += listing.own_bytes;
            d.tree.files += u64::from(listing.own_files);
            d.tree.dirs += u64::from(child_count);
            d.tree.last_used = d.tree.last_used.max(newest);
            at = d.parent;
        }
        snap.stats.files += u64::from(listing.own_files);
        snap.stats.bytes += listing.own_bytes;
        snap.stats.folders += u64::from(child_count);
        snap.stats.links_skipped += u64::from(listing.file_links);
        snap.stats.hard_links_merged += u64::from(listing.hard_links);
        if listing.flags.contains(DirFlags::UNREADABLE) {
            // Listed, but some entries could not be read (macOS: a folder readable without search permission).
            snap.stats.unreadable += 1;
        }
        first_child..first_child + child_count
    }

    pub(crate) fn finish(mut self, elapsed: Duration) -> Snapshot {
        self.snap.stats.elapsed = elapsed;
        self.snap.names.shrink_to_fit();
        self.snap.dirs.shrink_to_fit();
        self.snap.ids.shrink_to_fit();
        self.snap.files.shrink_to_fit();
        self.snap
    }
}

fn append_name(names: &mut Vec<u8>, bytes: &[u8]) -> NameRef {
    let start = names.len() as u32;
    names.extend_from_slice(bytes);
    NameRef {
        start,
        len: bytes.len() as u32,
    }
}
