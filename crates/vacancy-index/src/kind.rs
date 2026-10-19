//! What a listed entry is, from its attributes and reparse tag: the same rule the normal scan applies to a
//! directory listing (see `vacancy-core`'s `scan/windows.rs`), so both scans enter, skip and count the same things.

pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0000_0010;
pub const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x0000_0200;
pub const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
/// Reparse points that name somewhere else (junctions, symlinks, mount points) have this bit.
pub const NAME_SURROGATE: u32 = 0x2000_0000;
/// The cloud files family: `IO_REPARSE_TAG_CLOUD` and `IO_REPARSE_TAG_CLOUD_1` to `_F`.
const CLOUD_MASK: u32 = 0xFFFF_0FFF;
const CLOUD_TAG: u32 = 0x9000_001A;
/// Older OneDrive placeholders.
const ONEDRIVE_TAG: u32 = 0x8000_0021;
/// The Windows Overlay Filter's tag (CompactOS, `compact /exe`).
pub const WOF_TAG: u32 = 0x8000_0017;

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// A cloud-sync placeholder folder: entered.
    CloudDir,
    /// A folder-shaped link or other folder reparse point: recorded, never entered.
    DirLink,
    /// A file-shaped link: counted as a link, never followed.
    FileLink,
}

impl Kind {
    /// Whether a scan goes inside it.
    pub fn entered(self) -> bool {
        matches!(self, Kind::Dir | Kind::CloudDir)
    }
}

/// Whether a reparse tag is a cloud-sync placeholder's.
pub fn is_cloud(tag: u32) -> bool {
    tag & CLOUD_MASK == CLOUD_TAG || tag == ONEDRIVE_TAG
}

/// What an entry with these attributes and this tag is, as the normal scan decides it from a listing.
pub fn classify(attributes: u32, tag: u32) -> Kind {
    let is_dir = attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    let reparse = attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    let tag = if reparse { tag } else { 0 };
    match (is_dir, reparse) {
        (true, false) => Kind::Dir,
        (true, true) if is_cloud(tag) => Kind::CloudDir,
        (true, true) => Kind::DirLink,
        (false, true) if tag & NAME_SURROGATE != 0 => Kind::FileLink,
        (false, _) => Kind::File,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_as_the_scanner_does() {
        let d = FILE_ATTRIBUTE_DIRECTORY;
        let r = FILE_ATTRIBUTE_REPARSE_POINT;
        assert_eq!(classify(d, 0), Kind::Dir);
        assert_eq!(
            classify(d, 0xA000_0003),
            Kind::Dir,
            "a tag without the reparse attribute"
        );
        assert_eq!(classify(d | r, 0xA000_0003), Kind::DirLink, "a junction");
        assert_eq!(classify(d | r, 0x9000_F01A), Kind::CloudDir, "Dropbox");
        assert_eq!(classify(d | r, 0x8000_0021), Kind::CloudDir, "old OneDrive");
        assert_eq!(
            classify(d | r, 0x8000_0017),
            Kind::DirLink,
            "any other folder tag"
        );
        assert_eq!(classify(0x20, 0), Kind::File);
        assert_eq!(classify(0x20 | r, 0xA000_000C), Kind::FileLink, "a symlink");
        assert_eq!(classify(0x20 | r, WOF_TAG), Kind::File);
        assert!(Kind::CloudDir.entered() && Kind::Dir.entered());
        assert!(!Kind::DirLink.entered() && !Kind::File.entered());
    }
}
