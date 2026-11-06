#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "common/fixture_guard.rs"]
mod fixture_guard;
#[path = "../src/fixture/retention.rs"]
#[allow(dead_code)]
mod retention;
#[path = "prepared_launch/retention_support.rs"]
mod retention_support;
#[path = "prepared_launch/support.rs"]
mod support;

use hf_domain::process::ProcessExit;
use hf_platform::process::RetainedOwnedChild;
use std::{
    fs, io,
    time::{Duration, Instant},
};
use support::*;

fn invalid() -> io::Error {
    io::Error::other("private fixture refused")
}

#[test]
fn admitted_fixed_owner_is_retained_past_legacy_signal_window_then_settles_original_exit() {
    let _guard = fixture_guard::acquire();
    let scratch = scratch();
    let _cwd = CurrentDirectory::enter(&scratch);
    let (mut owner, mut peer) = retention_support::start_fixed_owner(&scratch);
    let release_at = Instant::now() + Duration::from_millis(2500);
    let mut final_exit = ProcessExit::Unobserved;
    retention::settle_observed(&mut owner, |exit| {
        final_exit = exit;
        if Instant::now() >= release_at {
            peer.release();
        }
    })
    .unwrap();
    assert!(
        Instant::now() >= release_at,
        "gate remained held past legacy signal window"
    );
    assert_eq!(
        final_exit,
        ProcessExit::Code(0),
        "retention observed original owner exit"
    );
    peer.finish().unwrap();
    fs::remove_dir_all(scratch).unwrap();
}
