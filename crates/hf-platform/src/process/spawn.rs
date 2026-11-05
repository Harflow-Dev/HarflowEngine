//! Safe Darwin spawn boundary; all child-visible descriptors are explicit.
use super::SpawnSpec;
use nix::{
    spawn::{PosixSpawnAttr, PosixSpawnFileActions, PosixSpawnFlags, posix_spawn},
    sys::signal::SigSet,
    unistd::Pid,
};
use std::{
    collections::BTreeSet,
    ffi::CString,
    fs::{self, File},
    io,
    os::{
        fd::{AsFd, AsRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    time::Instant,
};
pub(super) struct Spawned {
    pub pid: Pid,
    pub input: File,
    pub output: File,
    pub error: File,
    pub qualified: bool,
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "spawn qualification refused")
}
fn executable(spec: &SpawnSpec) -> io::Result<(u64, u64)> {
    let m = fs::symlink_metadata(&spec.executable)?;
    if !m.is_file()
        || m.mode() & 0o111 == 0
        || fs::canonicalize(&spec.executable)? != spec.executable
    {
        return Err(invalid());
    }
    Ok((m.dev(), m.ino()))
}
fn strings(spec: &SpawnSpec) -> io::Result<(Vec<CString>, Vec<CString>)> {
    let mut args =
        vec![CString::new(spec.executable.as_os_str().as_bytes()).map_err(|_| invalid())?];
    for arg in &spec.args {
        args.push(CString::new(arg.as_bytes()).map_err(|_| invalid())?);
    }
    let mut keys = BTreeSet::new();
    let mut env = Vec::new();
    for (key, value) in &spec.environment {
        let key = key.as_bytes();
        if key.is_empty() || key.contains(&b'=') || !keys.insert(key) {
            return Err(invalid());
        }
        let mut record = key.to_vec();
        record.push(b'=');
        record.extend_from_slice(value.as_bytes());
        env.push(CString::new(record).map_err(|_| invalid())?);
    }
    Ok((args, env))
}
fn pipe() -> io::Result<(File, File)> {
    let (read, write) = std::io::pipe()?;
    Ok((normalize(read.into())?, normalize(write.into())?))
}
fn normalize(fd: OwnedFd) -> io::Result<File> {
    if fd.as_raw_fd() >= 3 {
        return Ok(File::from(fd));
    }
    // BorrowedFd cloning uses F_DUPFD_CLOEXEC with a minimum descriptor of three.
    Ok(File::from(fd.as_fd().try_clone_to_owned()?))
}
pub(super) fn spawn(spec: &SpawnSpec, deadline: Instant) -> io::Result<Spawned> {
    let expected = executable(spec)?;
    if fs::canonicalize(&spec.cwd)? != fs::canonicalize(std::env::current_dir()?)? {
        return Err(invalid());
    }
    let (args, env) = strings(spec)?;
    let (input_child, input) = pipe()?;
    let (output, output_child) = pipe()?;
    let (error, error_child) = pipe()?;
    let mut actions = PosixSpawnFileActions::init()?;
    for (fd, target) in [(&input_child, 0), (&output_child, 1), (&error_child, 2)] {
        actions.add_dup2(fd.as_raw_fd(), target)?;
    }
    for fd in [
        &input_child,
        &input,
        &output,
        &output_child,
        &error,
        &error_child,
    ] {
        actions.add_close(fd.as_raw_fd())?;
    }
    let mut attr = PosixSpawnAttr::init()?;
    let flags = PosixSpawnFlags::POSIX_SPAWN_SETPGROUP
        | PosixSpawnFlags::POSIX_SPAWN_SETSIGDEF
        | PosixSpawnFlags::POSIX_SPAWN_SETSIGMASK
        | PosixSpawnFlags::from_bits_retain(nix::libc::POSIX_SPAWN_CLOEXEC_DEFAULT);
    attr.set_flags(flags)?;
    attr.set_pgroup(Pid::from_raw(0))?;
    attr.set_sigdefault(&SigSet::all())?;
    attr.set_sigmask(&SigSet::empty())?;
    if Instant::now() >= deadline {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "spawn deadline"));
    }
    if executable(spec)? != expected {
        return Err(invalid());
    }
    if Instant::now() >= deadline {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "spawn deadline"));
    }
    let pid = posix_spawn(&spec.executable, &actions, &attr, &args, &env)?;
    let qualified = executable(spec).is_ok_and(|actual| actual == expected);
    Ok(Spawned {
        pid,
        input,
        output,
        error,
        qualified,
    })
}
