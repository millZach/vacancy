//! Platforms Vacancy doesn't support delete nothing.

use std::ffi::OsStr;
use std::path::Path;

use super::{Entry, Fail, Open};
use crate::fsid::Step;

pub(crate) struct Dir;

pub(crate) struct Parent {
    pub(crate) dir: Dir,
    pub(crate) steps: Vec<Step>,
}

pub(crate) struct Busy;

impl Busy {
    pub(crate) fn now() -> Self {
        Busy
    }

    pub(crate) fn refresh(&mut self, _every: std::time::Duration) -> bool {
        false
    }

    pub(crate) fn holder(&self, _dev: u64, _ino: u128) -> Option<Option<String>> {
        None
    }
}

pub(crate) fn walk_to_parent(_path: &Path, _pin: bool) -> Result<Parent, Fail> {
    Err(Fail::Unsupported)
}

pub(crate) struct Held;

impl Held {
    pub(crate) fn still_there(&self) -> bool {
        false
    }
}

pub(crate) fn hold_file(
    _path: &Path,
    _expect: crate::fsid::Identity,
    _logical: u64,
    _alloc: u64,
    _threshold: i128,
) -> Result<Held, Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn entry(_parent: &Dir, _name: &OsStr) -> Result<Entry, Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn open_dir(
    _parent: &Dir,
    _name: &OsStr,
    _expect: Option<&Entry>,
    _mode: Open,
) -> Result<(Dir, Step), Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn changed(_dir: &Dir) -> Result<i128, Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn list(_dir: &Dir) -> Result<Vec<Entry>, Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn delete_file(
    _dir: &Dir,
    _e: &Entry,
    _threshold: i128,
    _busy: &Busy,
    _inside_target: bool,
) -> Result<Option<u64>, Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn delete_link(_dir: &Dir, _e: &Entry, _threshold: i128) -> Result<(), Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn remove_dir(
    _parent: &Dir,
    _dir: Dir,
    _step: &Step,
    _inside_target: bool,
) -> Result<(), Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn mounts_volume(_dir: &Dir, _e: &Entry) -> Result<bool, Fail> {
    Err(Fail::Unsupported)
}
