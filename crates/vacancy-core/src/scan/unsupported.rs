//! Platforms other than Windows and Unix. Vacancy does not support them.

use std::io;
use std::path::Path;

use super::{Calls, Collector, ListError, RootSpec};

#[derive(Clone, Debug)]
pub(crate) struct Volume;

impl Volume {
    pub(crate) fn id(&self) -> Option<u64> {
        None
    }
}

pub(crate) fn roots(_path: &Path) -> io::Result<Vec<RootSpec>> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[derive(Debug)]
pub(crate) struct Anchor;

pub(crate) fn opens_in_parent() -> bool {
    false
}

#[derive(Default)]
pub(crate) struct Lister {
    calls: Calls,
}

impl Lister {
    pub(crate) fn calls(&mut self) -> &mut Calls {
        &mut self.calls
    }

    pub(crate) fn list(
        &mut self,
        _dir: &Path,
        _parent: Option<&Anchor>,
        _expect: Option<u128>,
        _volume: &Volume,
        _out: &mut Collector<'_>,
    ) -> Result<Anchor, ListError> {
        Err(ListError::Failed)
    }
}
