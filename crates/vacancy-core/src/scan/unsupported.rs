//! Platforms other than Windows and Unix. Vacancy does not support them.

use std::io;
use std::path::Path;

use super::{Collector, ListError, RootSpec};

#[derive(Clone, Debug)]
pub(crate) struct Volume;

pub(crate) fn roots(_path: &Path) -> io::Result<Vec<RootSpec>> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[derive(Default)]
pub(crate) struct Lister;

impl Lister {
    pub(crate) fn list(
        &mut self,
        _dir: &Path,
        _expect: Option<u128>,
        _volume: &Volume,
        _out: &mut Collector<'_>,
    ) -> Result<(), ListError> {
        Err(ListError::Failed)
    }
}
