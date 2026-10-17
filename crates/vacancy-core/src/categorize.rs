//! The categorizer: sorts a [`Snapshot`] into the categories people see on the map (Apps & games, Developer,
//! Photos/videos/documents, System, Downloads, Caches & temp, Cloud sync, Other) and picks the items each
//! category shows.
//!
//! Rules come from a per-platform catalog of known locations, built from an [`Env`] (home, app data, temp, known
//! folders, cloud roots) so both platforms' catalogs can be tested anywhere, plus a few name rules (`node_modules`,
//! a Rust `target` next to `Cargo.toml`, a Git repository, browser `Cache` folders inside app data). A folder takes
//! the category of the nearest rule above it; every byte lands in exactly one category.
//!
//! System, the Photos library and cloud-sync roots are protected: shown, hatched, and never proposed for cleanup.
//! Inside a cloud root or a Photos library no other rule applies.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use ts_rs::TS;

use crate::snapshot::{DirFlags, DirId, FileId, Markers, RootKind, Snapshot, UnixTime};

mod catalog;
mod env;

pub use env::{CloudRoot, Env, KnownFolders, Platform};

/// The categories, in the order the map lists them when sizes tie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CategoryId {
    /// Installed programs and games.
    Apps,
    /// Code, project dependencies, build output, SDKs and containers.
    Dev,
    /// Personal files: documents, pictures, videos, music, the desktop.
    Media,
    /// The macOS Photos library. Protected.
    Photos,
    /// The OS. Protected.
    System,
    /// The Downloads folder.
    Downloads,
    /// Caches, temp files, logs and the Trash.
    Caches,
    /// Cloud-sync roots (OneDrive, Dropbox, iCloud Drive). Protected.
    Cloud,
    /// Everything else.
    Other,
}

impl CategoryId {
    /// Every category, in order.
    pub const ALL: [CategoryId; 9] = [
        Self::Apps,
        Self::Dev,
        Self::Media,
        Self::Photos,
        Self::System,
        Self::Downloads,
        Self::Caches,
        Self::Cloud,
        Self::Other,
    ];

    /// Whether Vacancy treats everything in the category as off-limits.
    pub fn protected(self) -> bool {
        matches!(self, Self::System | Self::Photos | Self::Cloud)
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One thing a category or a folder shows on the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemRef {
    /// A folder, with everything below it that belongs to the same item.
    Dir(DirId),
    /// A kept file.
    File(FileId),
    /// The files directly in a folder that are not kept individually (and, for a folder whose subfolders are
    /// items of their own, all of its files).
    FilesIn(DirId),
}

/// An item and its numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    /// What it is.
    pub at: ItemRef,
    /// Allocated bytes.
    pub bytes: u64,
    /// Files.
    pub files: u64,
    /// The most recent use of anything in it.
    pub last_used: UnixTime,
}

/// A category's totals and items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    /// Which category.
    pub id: CategoryId,
    /// Its name on this platform ("Windows & system", "macOS & System Data").
    pub name: String,
    /// One line about it.
    pub blurb: String,
    /// Whether Vacancy never touches it.
    pub protected: bool,
    /// Allocated bytes.
    pub bytes: u64,
    /// Files.
    pub files: u64,
    /// Its items, biggest first.
    pub items: Vec<Item>,
}

/// The result of [`categorize`].
#[derive(Debug)]
pub struct Categories {
    platform: Platform,
    list: Vec<Category>,
    node: Vec<Node>,
    residual: Vec<Sums>,
    file_rules: HashMap<FileId, CategoryId>,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    category: CategoryId,
    /// Starts an item: its category differs from its parent's, or its parent is a container.
    item_root: bool,
    /// Its subfolders are items of their own (C:\Program Files, the home folder).
    container: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Sums {
    bytes: u64,
    files: u64,
    last_used: UnixTime,
}

impl Sums {
    fn add(&mut self, other: Sums) {
        self.bytes += other.bytes;
        self.files += other.files;
        self.last_used = self.last_used.max(other.last_used);
    }
}

impl Categories {
    /// Whose rules sorted the snapshot.
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Every category, in [`CategoryId::ALL`] order, including empty ones.
    pub fn list(&self) -> &[Category] {
        &self.list
    }

    /// One category.
    pub fn get(&self, id: CategoryId) -> &Category {
        &self.list[id.index()]
    }

    /// The category a folder belongs to.
    pub fn category_of(&self, dir: DirId) -> CategoryId {
        self.node[dir.index()].category
    }

    /// The category a kept file belongs to.
    pub fn category_of_file(&self, snapshot: &Snapshot, file: FileId) -> CategoryId {
        self.file_rules
            .get(&file)
            .copied()
            .unwrap_or_else(|| self.category_of(snapshot.file(file).parent))
    }

    /// What a folder holds within its own item: subfolders and kept files that belong to it, and its other files
    /// as one entry. This is the map's drill-down: the contents add up to the folder's item. Biggest first.
    pub fn contents(&self, snapshot: &Snapshot, dir: DirId) -> Vec<Item> {
        let mut out = Vec::new();
        for child in snapshot.children(dir) {
            let node = self.node[child.index()];
            if node.item_root {
                continue; // shown elsewhere on the map
            }
            let sums = self.residual[child.index()];
            if sums.bytes > 0 || sums.files > 0 {
                out.push(Item {
                    at: ItemRef::Dir(child),
                    bytes: sums.bytes,
                    files: sums.files,
                    last_used: sums.last_used,
                });
            }
        }
        let own = snapshot.own(dir);
        let (mut rest_bytes, mut rest_files) = (own.bytes, own.files);
        for file in snapshot.files(dir) {
            let f = snapshot.file(file);
            rest_bytes = rest_bytes.saturating_sub(f.bytes);
            rest_files = rest_files.saturating_sub(1);
            if self.file_rules.contains_key(&file) {
                continue;
            }
            out.push(Item {
                at: ItemRef::File(file),
                bytes: f.bytes,
                files: 1,
                last_used: f.last_used,
            });
        }
        if rest_bytes > 0 || rest_files > 0 {
            out.push(Item {
                at: ItemRef::FilesIn(dir),
                bytes: rest_bytes,
                files: rest_files,
                last_used: own.last_used,
            });
        }
        out.sort_by_key(|item| std::cmp::Reverse(item.bytes));
        out
    }

    /// A folder's numbers as an item (what its map tile shows), or `None` if it is not part of one (a container).
    pub fn item_of(&self, dir: DirId) -> Option<Item> {
        let node = self.node[dir.index()];
        if node.container {
            return None;
        }
        let sums = self.residual[dir.index()];
        Some(Item {
            at: ItemRef::Dir(dir),
            bytes: sums.bytes,
            files: sums.files,
            last_used: sums.last_used,
        })
    }
}

/// Sorts `snapshot` into categories. Works on a finished snapshot or a scan's preview.
pub fn categorize(snapshot: &Snapshot, env: &Env) -> Categories {
    let rules = catalog::rules(env);
    let trie = Trie::build(&rules);
    let n = snapshot.dir_count();
    let mut node = vec![
        Node {
            category: CategoryId::Other,
            item_root: false,
            container: false,
        };
        n
    ];
    // Per folder: where it is in the trie of known locations, and what it inherits.
    let mut at = vec![Ctx::default(); n];
    let mut file_rules = HashMap::new();
    let mut cloud_names: Vec<String> = Vec::new();

    for root in snapshot.roots() {
        let i = root.dir.index();
        let mut ctx = Ctx {
            trie: trie.find(&root.shown),
            ..Ctx::default()
        };
        let mut n_root = Node {
            category: CategoryId::Other,
            item_root: true,
            container: true,
        };
        if root.kind == RootKind::SealedSystem {
            n_root = Node {
                category: CategoryId::System,
                item_root: true,
                container: false,
            };
            ctx.sealed = true;
        }
        if !ctx.sealed
            && let Some(rule) = ctx.trie.and_then(|t| trie.rule(t))
        {
            apply(&mut n_root, &mut ctx, rule);
            n_root.item_root = !n_root.container;
        }
        node[i] = n_root;
        at[i] = ctx;
    }

    // Parents come before children, so one pass decides every folder.
    for dir in snapshot.dirs() {
        let i = dir.index();
        let Some(parent) = snapshot.parent(dir) else {
            continue;
        };
        let p = node[parent.index()];
        let pctx = at[parent.index()];
        let name = snapshot.name(dir);
        let mut me = Node {
            category: p.category,
            item_root: p.container,
            container: false,
        };
        let mut ctx = Ctx {
            trie: pctx.trie.and_then(|t| trie.child(t, name)),
            ..pctx
        };
        let flags = snapshot.flags(dir);
        let mut matched = false;
        if pctx.sealed {
            // Inside a cloud root, a Photos library or macOS's sealed system volume nothing changes.
        } else if let Some(rule) = ctx.trie.and_then(|t| trie.rule(t)) {
            apply(&mut me, &mut ctx, rule);
            matched = rule.category.is_some();
            if rule.category == Some(CategoryId::Cloud) && !rule.label.is_empty() {
                cloud_names.push(rule.label.clone());
            }
        } else {
            if flags.contains(DirFlags::CLOUD) && p.category != CategoryId::Cloud {
                // A cloud-sync root no catalog rule names (a Dropbox or OneDrive folder somewhere unusual).
                me.category = CategoryId::Cloud;
                ctx.sealed = true;
                matched = true;
                cloud_names.push(name.to_string_lossy().into_owned());
            } else if let Some(category) =
                name_rule(env, snapshot, dir, parent, name, p.category, pctx)
            {
                me.category = category;
                ctx.sealed |= category == CategoryId::Photos;
                matched = true;
                if name.eq_ignore_ascii_case("common")
                    && snapshot.name(parent).eq_ignore_ascii_case("steamapps")
                {
                    me.container = true;
                }
            }
        }
        if matched && me.category != p.category {
            me.item_root = true;
        }
        if me.container {
            me.item_root = false;
        }
        node[i] = me;
        at[i] = ctx;
    }

    // Files the catalog names (Windows' page file): their own items, wherever their folder is.
    for dir in snapshot.dirs() {
        let ctx = at[dir.index()];
        let Some(t) = ctx.trie else { continue };
        if !trie.has_children(t) {
            continue;
        }
        for file in snapshot.files(dir) {
            if let Some(rule) = trie
                .child(t, snapshot.file(file).name)
                .and_then(|c| trie.rule(c))
                && let Some(category) = rule.category
            {
                file_rules.insert(file, category);
            }
        }
    }

    // Bottom-up: what each folder adds to its item, and the items themselves.
    let mut residual = vec![Sums::default(); n];
    let mut items: Vec<Vec<Item>> = vec![Vec::new(); CategoryId::ALL.len()];
    for dir in snapshot.dirs().rev() {
        let i = dir.index();
        let me = node[i];
        let own = snapshot.own(dir);
        let mut own_sums = Sums {
            bytes: own.bytes,
            files: own.files,
            last_used: own.last_used,
        };
        for file in snapshot.files(dir) {
            if let Some(&category) = file_rules.get(&file) {
                let f = snapshot.file(file);
                own_sums.bytes = own_sums.bytes.saturating_sub(f.bytes);
                own_sums.files = own_sums.files.saturating_sub(1);
                items[category.index()].push(Item {
                    at: ItemRef::File(file),
                    bytes: f.bytes,
                    files: 1,
                    last_used: f.last_used,
                });
            }
        }
        if me.container {
            if own_sums.bytes > 0 || own_sums.files > 0 {
                items[me.category.index()].push(Item {
                    at: ItemRef::FilesIn(dir),
                    bytes: own_sums.bytes,
                    files: own_sums.files,
                    last_used: own_sums.last_used,
                });
            }
            residual[i] = Sums::default();
            continue;
        }
        residual[i].add(own_sums);
        let sums = residual[i];
        if me.item_root || snapshot.parent(dir).is_none() {
            if sums.bytes > 0 || sums.files > 0 {
                items[me.category.index()].push(Item {
                    at: ItemRef::Dir(dir),
                    bytes: sums.bytes,
                    files: sums.files,
                    last_used: sums.last_used,
                });
            }
        } else if let Some(parent) = snapshot.parent(dir) {
            residual[parent.index()].add(sums);
        }
    }

    let list = CategoryId::ALL
        .iter()
        .zip(items)
        .map(|(&id, mut items)| {
            items.sort_by_key(|item| std::cmp::Reverse(item.bytes));
            let bytes = items.iter().map(|i| i.bytes).sum();
            let files = items.iter().map(|i| i.files).sum();
            let (name, blurb) = catalog::describe(id, env.platform, &cloud_names, files);
            Category {
                id,
                name,
                blurb,
                protected: id.protected(),
                bytes,
                files,
                items,
            }
        })
        .collect();

    Categories {
        platform: env.platform,
        list,
        node,
        residual,
        file_rules,
    }
}

/// What a folder passes down to its subfolders while categorizing.
#[derive(Clone, Copy, Debug, Default)]
struct Ctx {
    /// Its node in the trie of known locations, if it is on the way to one.
    trie: Option<usize>,
    /// Inside a cloud root or a Photos library: no rule changes anything below.
    sealed: bool,
    /// Inside app data (AppData\Local, ~/Library/Application Support): cache-named folders are caches.
    app_data: bool,
}

fn apply(node: &mut Node, ctx: &mut Ctx, rule: &catalog::Rule) {
    if let Some(category) = rule.category {
        node.category = category;
    }
    node.container = rule.container;
    ctx.app_data |= rule.app_data;
    ctx.sealed |= rule.seal;
}

/// Rules that go by a folder's name rather than its place. They only apply in places that hold people's own
/// things (Other, personal files, Downloads), never inside apps, the OS or caches.
fn name_rule(
    env: &Env,
    snapshot: &Snapshot,
    dir: DirId,
    parent: DirId,
    name: &OsStr,
    inherited: CategoryId,
    ctx: Ctx,
) -> Option<CategoryId> {
    // Called for most folders, so it compares without allocating.
    let name = name.to_str()?;
    let is = |other: &str| name.eq_ignore_ascii_case(other);
    // A Steam library anywhere is games.
    if is("steamapps") || (is("common") && snapshot.name(parent).eq_ignore_ascii_case("steamapps"))
    {
        return (!inherited.protected()).then_some(CategoryId::Apps);
    }
    if !matches!(
        inherited,
        CategoryId::Other | CategoryId::Media | CategoryId::Downloads
    ) {
        return None;
    }
    if env.platform == Platform::MacOs && ends_with_ignore_case(name, ".photoslibrary") {
        return Some(CategoryId::Photos);
    }
    let markers = snapshot.markers(dir);
    let dev = is("node_modules")
        || (is("target") && snapshot.markers(parent).contains(Markers::CARGO_TOML))
        || markers.contains(Markers::GIT)
        || markers.contains(Markers::PYVENV)
        || is("__pycache__")
        || is(".gradle")
        || is("DerivedData");
    if dev {
        return Some(CategoryId::Dev);
    }
    if ctx.app_data && inherited == CategoryId::Other && catalog::is_cache_name(name) {
        return Some(CategoryId::Caches);
    }
    None
}

fn ends_with_ignore_case(name: &str, suffix: &str) -> bool {
    name.len() >= suffix.len()
        && name.is_char_boundary(name.len() - suffix.len())
        && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// Known locations, keyed by path components compared without case.
struct Trie {
    nodes: Vec<TrieNode>,
}

#[derive(Default)]
struct TrieNode {
    children: HashMap<String, usize>,
    rule: Option<usize>,
}

impl Trie {
    fn build(rules: &[catalog::Rule]) -> TrieWithRules<'_> {
        let mut trie = Trie {
            nodes: vec![TrieNode::default()],
        };
        for (i, rule) in rules.iter().enumerate() {
            let mut at = 0;
            for key in keys(&rule.at) {
                at = match trie.nodes[at].children.get(&key) {
                    Some(&next) => next,
                    None => {
                        let next = trie.nodes.len();
                        trie.nodes.push(TrieNode::default());
                        trie.nodes[at].children.insert(key, next);
                        next
                    }
                };
            }
            // The first rule for a place wins; catalogs list the specific before the general.
            trie.nodes[at].rule.get_or_insert(i);
        }
        TrieWithRules { trie, rules }
    }
}

struct TrieWithRules<'r> {
    trie: Trie,
    rules: &'r [catalog::Rule],
}

impl TrieWithRules<'_> {
    fn find(&self, path: &Path) -> Option<usize> {
        let mut at = 0;
        for key in keys(path) {
            at = *self.trie.nodes[at].children.get(&key)?;
        }
        Some(at)
    }

    fn child(&self, at: usize, name: &OsStr) -> Option<usize> {
        let node = &self.trie.nodes[at];
        if node.children.is_empty() {
            return None;
        }
        node.children
            .get(&name.to_string_lossy().to_lowercase())
            .copied()
    }

    fn has_children(&self, at: usize) -> bool {
        !self.trie.nodes[at].children.is_empty()
    }

    fn rule(&self, at: usize) -> Option<&catalog::Rule> {
        self.trie.nodes[at].rule.map(|i| &self.rules[i])
    }
}

/// A path's components as trie keys: the drive or root, then each name, lowercased.
fn keys(path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for part in plain(path).components() {
        match part {
            Component::Prefix(p) => out.push(p.as_os_str().to_string_lossy().to_lowercase()),
            Component::RootDir => out.push(String::from("/")),
            Component::Normal(name) => out.push(name.to_string_lossy().to_lowercase()),
            Component::CurDir | Component::ParentDir => {}
        }
    }
    out
}

/// Strips a Windows `\\?\` prefix, so `\\?\C:\Users` and `C:\Users` are the same place.
fn plain(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}
