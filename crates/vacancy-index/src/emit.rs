//! Turning the table into the stream the app reads: the folders the app's account can open, listed from the root
//! down; the folders it can't, as totals only; and what NTFS keeps for itself.
//!
//! Which folders are locked is decided here, with the app's own account (see [`Access`]), so no name inside a
//! locked folder ever leaves the reader: a locked folder's own name is in its parent's listing, as it is in a
//! normal scan, and nothing below it is sent but three numbers.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Write};
use std::ops::Range;

use crate::kind::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SPARSE_FILE, Kind,
    WOF_TAG, classify,
};
use crate::table::{
    F_COUNTED, F_DIR, F_FAR_SD, F_IN_USE, F_INLINE_SD, F_NAMED, F_REPARSE, F_STANDARD, F_VISIBLE,
    F_WOF_STREAM, NONE, Table, filetime,
};
use crate::wire::{ENTRY_DIR, ENTRY_ENTER, ENTRY_LOCKED, Encoder, End, Locked, OutEntry};

/// The root folder's record.
pub const ROOT: u32 = 5;
/// `$Extend`, the folder of NTFS's newer metadata files.
const EXTEND: u32 = 11;
/// Records below this are NTFS's own files, hidden from listings.
const FIRST_USER_RECORD: u32 = 16;

/// A folder's security, for [`Access::can_list`].
#[derive(Clone, Copy, Debug)]
pub enum Security<'a> {
    /// A self-relative security descriptor, as stored on the volume.
    Descriptor(&'a [u8]),
    /// It couldn't be found: the folder counts as locked.
    Unknown,
}

/// Decides whether the app's account can list a folder: open it with `FILE_LIST_DIRECTORY`, as the normal scan
/// does.
pub trait Access {
    fn can_list(&mut self, security: Security<'_>) -> bool;

    /// Whether checking itself failed (not a folder refused: the check couldn't be made, say with a bad token).
    /// Then nothing is sent: a drive of "locked" folders would be wrong, so the app runs the normal scan.
    fn broken(&self) -> bool {
        false
    }
}

/// What was sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub dirs: u64,
    pub entries: u64,
    pub locked: u64,
    pub records_bytes: u64,
}

/// Per folder: not reached yet, listed, or inside the locked folder with this index (plus two).
const UNSEEN: u32 = 0;
const VISIBLE: u32 = 1;

/// Sends the table: see the module docs. `stop` is asked between folders; when it says so, nothing more is sent
/// (and no `End`).
pub fn emit<W: Write>(
    table: &mut Table,
    sds: &[u8],
    sds_index: &HashMap<u32, Range<usize>>,
    access: &mut dyn Access,
    out: &mut Encoder<W>,
    stop: &dyn Fn() -> bool,
) -> io::Result<Option<Summary>> {
    let n = table.len() as usize;
    if n <= ROOT as usize || !listable_dir(table, ROOT) {
        return Err(io::Error::other("the root folder's record isn't readable"));
    }
    let heads = link_children(table);
    let mut state = vec![UNSEEN; n];
    let mut verdicts: HashMap<u32, bool> = HashMap::new();
    let mut locked_tops: Vec<u32> = Vec::new();
    let mut summary = Summary::default();
    let mut name = Vec::new();

    // The root first: a normal scan that may not list it sees nothing below it, and neither may this.
    if !can_list(table, ROOT, sds, sds_index, access, &mut verdicts) || access.broken() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the account can't list the drive's root",
        ));
    }
    // Visible folders, parents first.
    state[ROOT as usize] = VISIBLE;
    let mut queue = VecDeque::from([ROOT]);
    while let Some(dir) = queue.pop_front() {
        if stop() {
            return Ok(None);
        }
        out.dir(table.frn(dir))?;
        let mut count = 0u64;
        let mut at = heads[dir as usize];
        while at != NONE {
            let nr = table.names[at as usize];
            at = nr.next_in_parent;
            let owner = nr.owner;
            let row = table.rows[owner as usize];
            let (attributes, tag, allocated) = listed(table, owner);
            let kind = classify(attributes, tag);
            let mut flags = 0u8;
            if row.flags & F_DIR != 0 {
                flags |= ENTRY_DIR;
                // A folder reached twice (a damaged index) is listed once, under the first name.
                if state[owner as usize] != UNSEEN {
                    continue;
                }
                if kind.entered() {
                    if can_list(table, owner, sds, sds_index, access, &mut verdicts) {
                        flags |= ENTRY_ENTER;
                        state[owner as usize] = VISIBLE;
                        queue.push_back(owner);
                    } else {
                        flags |= ENTRY_LOCKED;
                        state[owner as usize] = locked_tops.len() as u32 + 2;
                        locked_tops.push(owner);
                    }
                }
            }
            table.name_into(&nr, &mut name);
            let file = kind == Kind::File && row.flags & F_DIR == 0;
            out.entry(&OutEntry {
                frn: table.frn(owner),
                attributes,
                tag,
                logical: if file { row.logical } else { 0 },
                allocated: if file { allocated } else { 0 },
                modified: filetime(row.modified),
                accessed: filetime(row.accessed),
                flags,
                name: &name,
            })?;
            count += 1;
        }
        out.dir_end(table.frn(dir), count)?;
        summary.dirs += 1;
        summary.entries += count;
        if access.broken() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "folders' permissions couldn't be checked",
            ));
        }
    }

    // Inside each locked folder: every folder below it (never through a link), counted.
    let mut folders = vec![0u64; locked_tops.len()];
    for (k, &top) in locked_tops.iter().enumerate() {
        let mark = k as u32 + 2;
        folders[k] = 1;
        let mut queue = VecDeque::from([top]);
        while let Some(dir) = queue.pop_front() {
            let mut at = heads[dir as usize];
            while at != NONE {
                let nr = table.names[at as usize];
                at = nr.next_in_parent;
                let owner = nr.owner;
                if table.rows[owner as usize].flags & F_DIR == 0 || state[owner as usize] != UNSEEN
                {
                    continue;
                }
                let (attributes, tag, _) = listed(table, owner);
                if classify(attributes, tag).entered() {
                    state[owner as usize] = mark;
                    folders[k] += 1;
                    queue.push_back(owner);
                }
            }
        }
    }

    // Files: a file with a name in a listed folder is the app's to count; one found only in locked folders is
    // counted in the first locked folder it is in, once.
    for nr in &table.names {
        if nr.parent != NONE && state.get(nr.parent as usize) == Some(&VISIBLE) {
            table.rows[nr.owner as usize].flags |= F_VISIBLE;
        }
    }
    let mut totals = vec![(0u64, 0u64); locked_tops.len()];
    for i in 0..table.names.len() {
        let nr = table.names[i];
        let Some(&s) = state.get(nr.parent as usize) else {
            continue;
        };
        if s < 2 {
            continue;
        }
        let row = table.rows[nr.owner as usize];
        if row.flags & (F_DIR | F_VISIBLE | F_COUNTED) != 0 || !linked(table, i as u32) {
            continue;
        }
        let (attributes, tag, allocated) = listed(table, nr.owner);
        if classify(attributes, tag) != Kind::File {
            continue;
        }
        table.rows[nr.owner as usize].flags |= F_COUNTED;
        let t = &mut totals[(s - 2) as usize];
        t.0 = t.0.saturating_add(allocated);
        t.1 += 1;
    }
    for (k, &top) in locked_tops.iter().enumerate() {
        out.locked(&Locked {
            frn: table.frn(top),
            bytes: totals[k].0,
            files: totals[k].1,
            folders: folders[k],
        })?;
    }
    summary.locked = locked_tops.len() as u64;

    // What NTFS keeps for itself: its own files' data and everything under $Extend, and every attribute that
    // isn't a file's own data (gathered as the table was read).
    let mut records = table.extra_bytes;
    for i in 0..FIRST_USER_RECORD.min(table.len()) {
        let row = table.rows[i as usize];
        if row.flags & F_IN_USE != 0 {
            records = records.saturating_add(row.allocated);
        }
    }
    if (EXTEND as usize) < n && table.rows[EXTEND as usize].flags & F_IN_USE != 0 {
        let mut seen = vec![false; n];
        let mut queue = VecDeque::from([EXTEND]);
        seen[EXTEND as usize] = true;
        while let Some(dir) = queue.pop_front() {
            let mut at = heads[dir as usize];
            while at != NONE {
                let nr = table.names[at as usize];
                at = nr.next_in_parent;
                if std::mem::replace(&mut seen[nr.owner as usize], true) {
                    continue;
                }
                let row = table.rows[nr.owner as usize];
                records = records.saturating_add(row.allocated);
                if row.flags & F_DIR != 0 {
                    queue.push_back(nr.owner);
                }
            }
        }
    }
    out.records(records)?;
    summary.records_bytes = records;
    out.end(&End {
        dirs: summary.dirs,
        entries: summary.entries,
        locked: summary.locked,
    })?;
    Ok(Some(summary))
}

/// Whether record `i` is a folder whose base record was read.
fn listable_dir(table: &Table, i: u32) -> bool {
    let f = table.rows[i as usize].flags;
    f & (F_IN_USE | F_STANDARD | F_DIR) == F_IN_USE | F_STANDARD | F_DIR
}

/// Links every name that belongs in a listing into its folder's list: names of in-use records with their
/// standard information, in an in-use folder of the sequence the name expects, never NTFS's own files, and a
/// folder by one name only. Returns each folder's first name.
fn link_children(table: &mut Table) -> Vec<u32> {
    let n = table.len() as usize;
    let mut heads = vec![NONE; n];
    for i in 0..table.names.len() {
        let nr = table.names[i];
        let (owner, parent) = (nr.owner, nr.parent);
        if owner < FIRST_USER_RECORD || owner == parent || parent as usize >= n {
            continue;
        }
        let o = table.rows[owner as usize];
        if o.flags & (F_IN_USE | F_STANDARD) != F_IN_USE | F_STANDARD {
            continue;
        }
        if !listable_dir(table, parent)
            || table.rows[parent as usize].sequence != nr.parent_sequence
        {
            continue;
        }
        if o.flags & F_DIR != 0 {
            if o.flags & F_NAMED != 0 {
                continue;
            }
            table.rows[owner as usize].flags |= F_NAMED;
        }
        table.names[i].next_in_parent = heads[parent as usize];
        heads[parent as usize] = i as u32;
    }
    heads
}

/// Whether name `i` was linked into a folder (see [`link_children`]).
fn linked(table: &Table, i: u32) -> bool {
    let nr = table.names[i as usize];
    nr.owner >= FIRST_USER_RECORD
        && table.rows[nr.owner as usize].flags & (F_IN_USE | F_STANDARD) == F_IN_USE | F_STANDARD
        && (nr.parent as usize) < table.rows.len()
        && listable_dir(table, nr.parent)
        && table.rows[nr.parent as usize].sequence == nr.parent_sequence
}

/// A record's attributes, reparse tag and allocation as a directory listing shows them. A file the Windows
/// Overlay Filter keeps compressed lists as a plain file (the filter hides its tag), and its allocation is its
/// compressed data's, as Windows reports it by handle.
fn listed(table: &Table, i: u32) -> (u32, u32, u64) {
    let row = table.rows[i as usize];
    let mut attributes = row.attributes;
    if row.flags & F_DIR != 0 {
        attributes |= FILE_ATTRIBUTE_DIRECTORY;
    }
    let mut tag = 0;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        // A reparse point whose tag can't be read here is taken for a link: never entered.
        tag = if row.flags & F_REPARSE != 0 {
            table.tags.get(&i).copied().unwrap_or(u32::MAX)
        } else {
            u32::MAX
        };
    }
    let mut allocated = row.allocated;
    if tag == WOF_TAG && row.flags & F_DIR == 0 {
        attributes &= !(FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_SPARSE_FILE);
        tag = 0;
        if row.flags & F_WOF_STREAM != 0 {
            allocated = table.wof.get(&i).copied().unwrap_or(allocated);
        }
    }
    (attributes, tag, allocated)
}

fn can_list(
    table: &Table,
    dir: u32,
    sds: &[u8],
    sds_index: &HashMap<u32, Range<usize>>,
    access: &mut dyn Access,
    verdicts: &mut HashMap<u32, bool>,
) -> bool {
    let row = table.rows[dir as usize];
    if row.flags & F_INLINE_SD != 0 {
        let sd = table.inline_sd.get(&dir).map(Vec::as_slice);
        return access.can_list(sd.map_or(Security::Unknown, Security::Descriptor));
    }
    if row.flags & F_FAR_SD != 0 {
        return access.can_list(Security::Unknown);
    }
    let id = row.security_id;
    if let Some(&v) = verdicts.get(&id) {
        return v;
    }
    let security = sds_index
        .get(&id)
        .and_then(|r| sds.get(r.clone()))
        .map_or(Security::Unknown, Security::Descriptor);
    let v = access.can_list(security);
    verdicts.insert(id, v);
    v
}
