//! The kernel's own spelling of an existing path: the case and Unicode form the filesystem
//! stores, with firmlink aliases collapsed. On Darwin `F_GETPATH` on an event-only
//! descriptor reports it; the no-follow form keeps a symlink's own name instead of its
//! target's. The descriptor is never read or written.
use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl, open},
    libc,
    sys::stat::Mode,
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

pub(super) enum Spelling {
    Found(PathBuf),
    /// The path itself does not exist.
    Missing,
}

/// `None` for every failure other than absence, including a path or spelling that is not
/// UTF-8: nothing is guessed. The spelling comes from the kernel and `nix` unwraps it as
/// UTF-8, so a non-UTF-8 result panics inside `nix`; that is contained here and reads as a
/// failed lookup. The input is checked first. The containment relies on unwinding: under a
/// `panic = "abort"` profile it would be a crash instead.
pub(super) fn kernel_spelling(path: &Path, follow: bool) -> Option<Spelling> {
    path.to_str()?;
    let mut flags = OFlag::from_bits_retain(libc::O_EVTONLY) | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC;
    if !follow {
        flags |= OFlag::from_bits_retain(libc::O_SYMLINK);
    }
    let descriptor = match open(path, flags, Mode::empty()) {
        Ok(descriptor) => descriptor,
        Err(Errno::ENOENT) => return Some(Spelling::Missing),
        Err(_) => return None,
    };
    let mut spelled = PathBuf::new();
    catch_unwind(AssertUnwindSafe(|| {
        fcntl(&descriptor, FcntlArg::F_GETPATH(&mut spelled))
    }))
    .ok()?
    .ok()?;
    spelled.is_absolute().then_some(Spelling::Found(spelled))
}
