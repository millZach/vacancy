//! Platforms Vacancy doesn't support delete nothing.

use std::ffi::OsStr;
use std::path::Path;

use super::{Entry, Fail};
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
}

pub(crate) fn walk_to_parent(_path: &Path) -> Result<Parent, Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn entry(_parent: &Dir, _name: &OsStr) -> Result<Entry, Fail> {
    Err(Fail::Unsupported)
}

pub(crate) fn open_dir(
    _parent: &Dir,
    _name: &OsStr,
    _expect: Option<&Entry>,
    _delete: bool,
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
