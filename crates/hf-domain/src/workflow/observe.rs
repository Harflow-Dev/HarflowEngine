//! Rules on observed repository paths that need no filesystem.

/// A saved definition holds at most this many observed repositories.
pub const MAX_STORED: usize = 7;

/// Python's `Path(text).is_absolute()` on a POSIX host.
pub fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
}
