#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_domain::process::ProcessExit;
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnSpec},
    qualification::{
        FixtureRoot,
        live::{TwoTunnelFailureReason, TwoTunnelProxy},
    },
};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream},
    time::{Duration, Instant},
};

#[path = "claude_live/actors.rs"]
#[allow(dead_code)]
mod actors;
#[path = "claude_two_tunnel/codec.rs"]
#[allow(dead_code)]
mod codec;

#[path = "claude_transport_local/support.rs"]
mod support;

#[test]
fn local_tls_trust_differential_direct_and_proxy() {
    support::direct_pair();
    support::trusted_proxy_pair();
}

#[test]
fn local_curl_default_verification_rejects_fixture_via_proxy() {
    support::curl_negative();
}
