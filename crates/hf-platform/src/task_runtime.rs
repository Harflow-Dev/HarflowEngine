//! Production owner mechanisms. No public protocol or backend selection lives here.
pub mod bootstrap;
pub mod caller;
pub mod channel;
pub mod raw;
/// Enter a detached session at the fresh, single-thread owner entry point.
/// This is deliberately not a child pre-exec hook and never signals a group.
pub fn enter_owner_session() -> std::io::Result<u32> {
    nix::unistd::setsid()
        .map(|pid| pid.as_raw() as u32)
        .map_err(std::io::Error::from)
}
