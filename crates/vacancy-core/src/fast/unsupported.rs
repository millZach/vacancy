//! Fast scan reads NTFS's file index, which only Windows has: elsewhere there is no reader to start.

use std::io::Read;

use crate::scan::CancelToken;

/// Why the reader didn't send anything.
#[derive(Debug)]
pub(crate) enum Launch {
    /// The admin prompt was answered No, or closed.
    #[allow(dead_code, reason = "only Windows has an admin prompt")]
    Declined,
    /// The reader couldn't be started.
    Failed(String),
}

pub(crate) fn read_index<R>(
    _letter: char,
    _owner: Option<isize>,
    _cancel: &CancelToken,
    _read: impl FnOnce(&mut dyn Read) -> R,
) -> Result<R, Launch> {
    Err(Launch::Failed("fast scan is Windows only".into()))
}
