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
use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use crate::pathkey::{PathKey, name_key_in};
use crate::snapshot::{DirFlags, DirId, FileId, Markers, RootKind, Snapshot, UnixTime};

mod catalog;
mod env;

pub use crate::pathkey::Case;
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
    /// Everything one cleanup proposal would remove, gathered into one tile: its index in the
    /// [`Proposals`](crate::Proposals) the categories were [carved](Categories::carve) with.
    Proposal(usize),
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
    /// What proposals took out of their items, once carved.
    carve: Option<Carve>,
}

/// One proposal as the carve sees it: its category and what it takes.
pub(crate) type Claim<'a> = (CategoryId, &'a [ItemRef]);

/// What each proposal takes, by place. A value is the proposal's index plus one.
#[derive(Debug, Default)]
struct Carve {
    /// Per folder: the proposal that takes it whole, also set on every folder below it.
    dir: Vec<u32>,
    /// Kept files a proposal takes.
    files: HashMap<FileId, u32>,
    /// Folders whose loose files a proposal takes.
    loose: HashMap<DirId, u32>,
}

impl Carve {
    fn dir(&self, dir: DirId) -> bool {
        self.dir[dir.index()] != 0
    }
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
            if node.item_root || self.taken_dir(child) {
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
            if self.taken_file(file) || self.file_rules.contains_key(&file) {
                continue;
            }
            out.push(Item {
                at: ItemRef::File(file),
                bytes: f.bytes,
                files: 1,
                last_used: f.last_used,
            });
        }
        if self.taken_loose(dir) {
            (rest_bytes, rest_files) = (0, 0);
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

    /// Whether opening a folder's tile would show anything but its loose files: a subfolder or a kept file that
    /// is part of its item. Stops early on the first one; `None` if `stop` says so (asked every [`CHECK_EVERY`]
    /// subfolders, so a very wide folder doesn't hold up a cancel).
    pub(crate) fn has_parts(
        &self,
        snapshot: &Snapshot,
        dir: DirId,
        stop: &dyn Fn() -> bool,
    ) -> Option<bool> {
        for (n, child) in snapshot.children(dir).enumerate() {
            if n % CHECK_EVERY == 0 && stop() {
                return None;
            }
            let sums = self.residual[child.index()];
            if !self.node[child.index()].item_root
                && !self.taken_dir(child)
                && (sums.bytes > 0 || sums.files > 0)
            {
                return Some(true);
            }
        }
        Some(
            snapshot
                .files(dir)
                .any(|f| !self.file_rules.contains_key(&f) && !self.taken_file(f)),
        )
    }

    /// A folder's numbers as an item (what its map tile shows), or `None` if it is not part of one (a container).
    pub fn item_of(&self, dir: DirId) -> Option<Item> {
        let node = self.node[dir.index()];
        if node.container || self.taken_dir(dir) {
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

impl Categories {
    /// Moves what each proposal would remove out of the items it was in and into one item per proposal, in the
    /// proposal's category: `claims[i]` is proposal `i`'s category and targets, which must not overlap. Every byte
    /// stays in exactly one item, and folders' drill-downs leave out what a proposal took. `None` if `stop` says
    /// so.
    pub(crate) fn carve(
        mut self,
        snapshot: &Snapshot,
        claims: &[Claim<'_>],
        stop: &dyn Fn() -> bool,
    ) -> Option<Self> {
        let mut carve = Carve {
            dir: vec![0; snapshot.dir_count()],
            ..Carve::default()
        };
        for (p, (_, targets)) in claims.iter().enumerate() {
            let mark = u32::try_from(p + 1).ok()?;
            for &t in *targets {
                match t {
                    ItemRef::Dir(d) => carve.dir[d.index()] = mark,
                    ItemRef::File(f) => {
                        carve.files.insert(f, mark);
                    }
                    ItemRef::FilesIn(d) => {
                        carve.loose.insert(d, mark);
                    }
                    ItemRef::Proposal(_) => {}
                }
            }
        }
        // Parents come before children: a folder inside a taken one is taken by the same proposal.
        for dir in snapshot.dirs() {
            if dir.index() % CHECK_EVERY == 0 && stop() {
                return None;
            }
            if carve.dir[dir.index()] == 0
                && let Some(parent) = snapshot.parent(dir)
            {
                carve.dir[dir.index()] = carve.dir[parent.index()];
            }
        }
        let built = build_items(
            snapshot,
            &self.node,
            &self.file_rules,
            Some((&carve, claims)),
            stop,
        )?;
        let names: Vec<(String, String)> = self
            .list
            .iter()
            .map(|c| (c.name.clone(), c.blurb.clone()))
            .collect();
        self.list = finish_list(built.items, |id, _| names[id.index()].clone());
        self.residual = built.residual;
        self.carve = Some(carve);
        Some(self)
    }

    fn taken_dir(&self, dir: DirId) -> bool {
        self.carve.as_ref().is_some_and(|c| c.dir(dir))
    }

    fn taken_file(&self, file: FileId) -> bool {
        self.carve
            .as_ref()
            .is_some_and(|c| c.files.contains_key(&file))
    }

    fn taken_loose(&self, dir: DirId) -> bool {
        self.carve
            .as_ref()
            .is_some_and(|c| c.loose.contains_key(&dir))
    }
}

/// Folders between checks for a stop request while categorizing or building the map.
pub(crate) const CHECK_EVERY: usize = 1024;

/// Sorts `snapshot` into categories. Works on a finished snapshot or a scan's preview.
pub fn categorize(snapshot: &Snapshot, env: &Env) -> Categories {
    categorize_until(snapshot, env, &|| false).expect("categorizing that is never stopped finishes")
}

/// [`categorize`], giving up (`None`) as soon as `stop` says so. `stop` is asked every [`CHECK_EVERY`] folders
/// of each pass, so a cancelled scan's map stops being built within a few milliseconds.
pub(crate) fn categorize_until(
    snapshot: &Snapshot,
    env: &Env,
    stop: &dyn Fn() -> bool,
) -> Option<Categories> {
    let rules = catalog::rules(env);
    let trie = Trie::build(&rules, env.case);
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
        if i % CHECK_EVERY == 0 && stop() {
            return None;
        }
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
            packages: false,
            libs: pctx.libs || is_library_folder(name),
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
        if dir.index() % CHECK_EVERY == 0 && stop() {
            return None;
        }
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

    let built = build_items(snapshot, &node, &file_rules, None, stop)?;
    let list = finish_list(built.items, |id, files| {
        catalog::describe(id, env.platform, &cloud_names, files)
    });

    Some(Categories {
        platform: env.platform,
        list,
        node,
        residual: built.residual,
        file_rules,
        carve: None,
    })
}

/// What the bottom-up pass makes: what each folder adds to its item, and each category's items.
struct Built {
    residual: Vec<Sums>,
    items: Vec<Vec<Item>>,
}

/// Bottom-up: what each folder adds to its item, and the items themselves. With a carve, what a proposal takes goes
/// to the proposal's item (in its category) instead of the item it was in.
fn build_items(
    snapshot: &Snapshot,
    node: &[Node],
    file_rules: &HashMap<FileId, CategoryId>,
    carve: Option<(&Carve, &[Claim<'_>])>,
    stop: &dyn Fn() -> bool,
) -> Option<Built> {
    let n = snapshot.dir_count();
    let mut residual = vec![Sums::default(); n];
    let mut items: Vec<Vec<Item>> = vec![Vec::new(); CategoryId::ALL.len()];
    let mut taken = vec![Sums::default(); carve.map_or(0, |(_, claims)| claims.len())];
    for dir in snapshot.dirs().rev() {
        let i = dir.index();
        if i % CHECK_EVERY == 0 && stop() {
            return None;
        }
        let me = node[i];
        let own = snapshot.own(dir);
        let mut own_sums = Sums {
            bytes: own.bytes,
            files: own.files,
            last_used: own.last_used,
        };
        if let Some((c, _)) = carve
            && c.dir[i] != 0
        {
            // Taken whole, with everything below it (which added its own files the same way).
            taken[c.dir[i] as usize - 1].add(own_sums);
            continue;
        }
        for file in snapshot.files(dir) {
            let f = snapshot.file(file);
            let sums = Sums {
                bytes: f.bytes,
                files: 1,
                last_used: f.last_used,
            };
            // A proposal's claim comes first, so its tile is always as big as the proposal says.
            if let Some((c, _)) = carve
                && let Some(&p) = c.files.get(&file)
            {
                own_sums.bytes = own_sums.bytes.saturating_sub(f.bytes);
                own_sums.files = own_sums.files.saturating_sub(1);
                taken[p as usize - 1].add(sums);
            } else if let Some(&category) = file_rules.get(&file) {
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
        if let Some((c, _)) = carve
            && let Some(&p) = c.loose.get(&dir)
        {
            let (mut bytes, mut files) = (own.bytes, own.files);
            for f in snapshot.files(dir) {
                bytes = bytes.saturating_sub(snapshot.file(f).bytes);
                files = files.saturating_sub(1);
            }
            own_sums.bytes = own_sums.bytes.saturating_sub(bytes);
            own_sums.files = own_sums.files.saturating_sub(files);
            taken[p as usize - 1].add(Sums {
                bytes,
                files,
                last_used: own.last_used,
            });
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
    if let Some((_, claims)) = carve {
        for (p, sums) in taken.into_iter().enumerate() {
            if sums.bytes > 0 || sums.files > 0 {
                items[claims[p].0.index()].push(Item {
                    at: ItemRef::Proposal(p),
                    bytes: sums.bytes,
                    files: sums.files,
                    last_used: sums.last_used,
                });
            }
        }
    }
    Some(Built { residual, items })
}

/// The categories from their items, biggest item first, named by `describe` (from the category and its files).
fn finish_list(
    items: Vec<Vec<Item>>,
    describe: impl Fn(CategoryId, u64) -> (String, String),
) -> Vec<Category> {
    CategoryId::ALL
        .iter()
        .zip(items)
        .map(|(&id, mut items)| {
            items.sort_by_key(|item| std::cmp::Reverse(item.bytes));
            let bytes = items.iter().map(|i| i.bytes).sum();
            let files = items.iter().map(|i| i.files).sum();
            let (name, blurb) = describe(id, files);
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
        .collect()
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
    /// This is Windows' Store packages folder: subfolders named after a Linux distribution hold WSL disks.
    packages: bool,
    /// Inside a folder of code libraries (`node_modules`, `site-packages`): a folder named `cache` there is a
    /// library, not a cache.
    libs: bool,
}

fn apply(node: &mut Node, ctx: &mut Ctx, rule: &catalog::Rule) {
    if let Some(category) = rule.category {
        node.category = category;
    }
    node.container = rule.container;
    ctx.app_data |= rule.app_data;
    ctx.sealed |= rule.seal;
    ctx.packages = rule.packages;
}

/// Rules that go by a folder's name rather than its place. A Photos library is protected anywhere outside an
/// already protected place, and a Steam library is games anywhere; the rest only apply in places that hold
/// people's own things (Other, personal files, Downloads), never inside apps, the OS or caches.
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
    // A Photos library is protected wherever it is: in the Trash, inside an app, among developer files.
    if env.platform == Platform::MacOs
        && ends_with_ignore_case(name, ".photoslibrary")
        && !inherited.protected()
    {
        return Some(CategoryId::Photos);
    }
    // A Steam library anywhere is games.
    if is("steamapps") || (is("common") && snapshot.name(parent).eq_ignore_ascii_case("steamapps"))
    {
        return (!inherited.protected()).then_some(CategoryId::Apps);
    }
    // Caches inside app data are caches, whichever app or tool keeps them.
    if ctx.app_data
        && !ctx.libs
        && matches!(
            inherited,
            CategoryId::Other | CategoryId::Dev | CategoryId::Apps
        )
        && catalog::is_cache_name(name)
    {
        return Some(CategoryId::Caches);
    }
    // A Linux distribution installed from the Store keeps its WSL disk in its package folder.
    if ctx.packages && catalog::is_linux_package(name) {
        return Some(CategoryId::Dev);
    }
    if !matches!(
        inherited,
        CategoryId::Other | CategoryId::Media | CategoryId::Downloads
    ) {
        return None;
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
    None
}

/// Folders that hold code libraries, where names like `cache` are packages rather than caches.
pub(crate) fn is_library_folder(name: &OsStr) -> bool {
    let is = |other: &str| name.eq_ignore_ascii_case(other);
    is("node_modules") || is("site-packages") || is("dist-packages")
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
    fn build(rules: &[catalog::Rule], case: Case) -> TrieWithRules<'_> {
        let mut trie = Trie {
            nodes: vec![TrieNode::default()],
        };
        for (i, rule) in rules.iter().enumerate() {
            // A place with no key (relative, or with `..`) can't be matched; it names nothing on the volume.
            let Some(key) = PathKey::of(&rule.at) else {
                continue;
            };
            let mut at = 0;
            for key in key.keys_in(case).iter().cloned() {
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
        TrieWithRules { trie, rules, case }
    }
}

struct TrieWithRules<'r> {
    trie: Trie,
    rules: &'r [catalog::Rule],
    /// How the volume compares names: known places match exact spellings on a case-sensitive one.
    case: Case,
}

impl TrieWithRules<'_> {
    fn find(&self, path: &Path) -> Option<usize> {
        let mut at = 0;
        for key in PathKey::of(path)?.keys_in(self.case) {
            at = *self.trie.nodes[at].children.get(key)?;
        }
        Some(at)
    }

    fn child(&self, at: usize, name: &OsStr) -> Option<usize> {
        let node = &self.trie.nodes[at];
        if node.children.is_empty() {
            return None;
        }
        node.children.get(&name_key_in(name, self.case)).copied()
    }

    fn has_children(&self, at: usize) -> bool {
        !self.trie.nodes[at].children.is_empty()
    }

    fn rule(&self, at: usize) -> Option<&catalog::Rule> {
        self.trie.nodes[at].rule.map(|i| &self.rules[i])
    }
}
