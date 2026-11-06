//! Source-owned fixed-image witness for the generic safe exec adapter only.
use super::*;
use std::{os::unix::process::CommandExt, process::Command};
pub(crate) const BRIDGE: &str = "--fixture-exec-bridge-witness-v1";
pub(crate) const IMAGE: &str = "--fixture-exec-image-witness-v1";
pub(super) fn run(channel: PrivateIo, until: Instant, mode: &str) -> io::Result<()> {
    if mode == IMAGE {
        check_entry()?;
        hf_platform::lease::check_exec_image()?;
        channel.expect_eof()?;
        send(&channel, &Frame::Hello(self_identity()?))?;
        let cwd = std::env::current_dir()?;
        let bytes = cwd.to_str().ok_or_else(invalid)?.as_bytes();
        hf_platform::fixture::write_all(&std::io::stdout(), bytes, until)?;
        return Ok(());
    }
    if mode != BRIDGE {
        return Err(invalid());
    }
    let Frame::Start(start) = receive(&channel)? else {
        return Err(invalid());
    };
    send(&channel, &Frame::Hello(self_identity()?))?;
    channel.expect_eof()?;
    let parent = root_parent(&start.root)?;
    if parent != std::env::current_dir()? || std::fs::canonicalize(&parent)? != parent {
        return Err(invalid());
    }
    {
        let hold = map(hf_storage::RootHold::open(
            &start.root,
            until,
            hf_platform::lease::shared(),
        ))?;
        if hold.source_stamp().generation != start.fence.generation {
            return Err(invalid());
        }
    }
    let executable = std::env::current_exe()?;
    let mut command = Command::new(executable);
    command.env_clear().arg(IMAGE);
    hf_platform::lease::restore_blocking_stdio()?;
    let _barrier = hf_platform::lease::exec_barrier()?;
    Err(command.exec())
}
