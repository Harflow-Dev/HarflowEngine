#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::owner_runtime::DigestPort;
use hf_platform::qualification::{FixtureRoot, ProfilePolicy};
use std::{
    fs::{self, Metadata, OpenOptions},
    io::Write,
    path::PathBuf,
};

use super::{SettledRoot, canary, identity, invocation, server};

pub(super) const PYTHON: &str = "/Library/Developer/CommandLineTools/usr/bin/python3";
const PYTHON_RUNTIME: &str =
    "/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework";
pub(super) const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_measurement/preflight.py"
);
const OS_BANNER_RULE: &str = "(allow sysctl-read (sysctl-name \"kern.osproductversion\"))\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MeasurementMode {
    FullFixture,
    VersionOnlySysctl,
}

struct Measurement {
    selection: identity::VendorSelection,
    owned: SettledRoot,
    nonce: String,
    canary: PathBuf,
    canary_identity: Metadata,
    positive: PathBuf,
    server: server::FixtureServer,
    denied_listener: server::DeniedListener,
    controlled_profile: String,
    vendor_profile: String,
}

pub(super) fn run(mode: MeasurementMode) {
    let measurement = Measurement::create(mode);
    measurement.prove_preflight();
    measurement.invoke(mode);
    measurement.finish(mode);
}

impl Measurement {
    fn create(mode: MeasurementMode) -> Self {
        let selection = identity::selection();
        let owned = SettledRoot::create();
        let root = &owned.fixture;
        let nonce = identity::nonce_from_root(root.root());
        let (canary, canary_identity, positive) = canaries(root, &nonce);
        invocation::prepare_private_dirs(root);
        let server = server::FixtureServer::start(nonce.clone())
            .expect("reserve same-port IPv4 and IPv6 loopback listeners");
        let denied_listener = server::DeniedListener::bind(server.port()).unwrap();
        let (controlled_profile, base_vendor) = profiles(root, &selection, server.port());
        let vendor_profile = select_profile(mode, &base_vendor);
        eprintln!(
            "phase=profile-created mode={mode:?} root-identity={:?} base-profile-sha256={} selected-profile-sha256={} diagnostic-delta={}",
            root.identity(),
            hash(&base_vendor),
            hash(&vendor_profile),
            delta(mode)
        );
        Self {
            selection,
            owned,
            nonce,
            canary,
            canary_identity,
            positive,
            server,
            denied_listener,
            controlled_profile,
            vendor_profile,
        }
    }

    fn prove_preflight(&self) {
        let root = &self.owned.fixture;
        eprintln!("phase=controlled-posix-spawn-positive begin");
        assert_eq!(
            super::launch_spawn_control(root, &self.controlled_profile),
            "spawn-control=allowed"
        );
        eprintln!("phase=controlled-posix-spawn-positive complete");
        eprintln!("phase=vendor-nofork-read-network-preflight begin");
        let result = super::launch_preflight(
            root,
            &self.vendor_profile,
            &self.canary,
            &self.positive,
            self.server.port(),
            self.denied_listener.port(),
            &self.nonce,
        );
        assert_eq!(
            result,
            "fork=1 posix_spawn=1 read=1 inside=1 allowed=1 denied_port=1"
        );
        eprintln!("phase=vendor-nofork-read-network-preflight complete");
    }

    fn invoke(&self, mode: MeasurementMode) {
        let root = &self.owned.fixture;
        match mode {
            MeasurementMode::FullFixture => {
                eprintln!("phase=vendor-version-and-fixture begin");
                invocation::launch_vendor(
                    root,
                    &self.vendor_profile,
                    &self.selection,
                    self.server.port(),
                    &self.nonce,
                );
                eprintln!("phase=vendor-version-and-fixture complete");
            }
            MeasurementMode::VersionOnlySysctl => {
                eprintln!("phase=vendor-version-only-sysctl begin");
                invocation::launch_version_only(
                    root,
                    &self.vendor_profile,
                    &self.selection,
                    self.server.port(),
                    &self.nonce,
                );
                eprintln!(
                    "phase=vendor-version-only-sysctl complete; messages route was not invoked"
                );
            }
        }
    }

    fn finish(self, mode: MeasurementMode) {
        assert!(
            self.server.no_ipv6_connection(),
            "localhost scope's IPv6 endpoint received no connection"
        );
        eprintln!(
            "localhost profile scope covered 127.0.0.1:{} and ::1:{}; fixture HTTP was IPv4 only",
            self.server.port(),
            self.server.port()
        );
        assert!(
            self.denied_listener.no_connection(),
            "unlisted IPv4 loopback port received no connection"
        );
        let expected = if mode == MeasurementMode::FullFixture {
            server::ServerOutcome::Completed
        } else {
            server::ServerOutcome::PreflightOnly
        };
        let Self {
            selection,
            mut owned,
            nonce: _,
            canary,
            canary_identity,
            positive: _,
            mut server,
            denied_listener,
            controlled_profile: _,
            vendor_profile: _,
        } = self;
        drop(denied_listener);
        assert_eq!(server.stop_and_join(), expected);
        drop(server);
        canary::verify_and_remove(&canary, &canary_identity);
        owned.settle();
        eprintln!(
            "Claude {}/2.1.318 selected; SHA-256 {}; measurement settled",
            selection.logical.display(),
            selection.expected_hash
        );
    }
}

fn canaries(root: &FixtureRoot, nonce: &str) -> (PathBuf, Metadata, PathBuf) {
    let canary = root
        .root()
        .parent()
        .expect("fixture temp parent")
        .join(format!("{nonce}-claude-read-canary"));
    let positive = root.workspace().join("read-positive");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&canary)
        .expect("unique outside read canary");
    file.write_all(b"outside-user-data").unwrap();
    file.sync_all().unwrap();
    let identity = fs::symlink_metadata(&canary).unwrap();
    drop(file);
    fs::write(&positive, b"Inside-workspace-positive-control").unwrap();
    (canary, identity, positive)
}

fn profiles(
    root: &FixtureRoot,
    selection: &identity::VendorSelection,
    port: u16,
) -> (String, String) {
    let reads = [
        PathBuf::from(PYTHON),
        PathBuf::from(PYTHON_RUNTIME),
        PathBuf::from(HELPER),
        PathBuf::from("/usr/bin/true"),
        selection.logical.clone(),
    ];
    (
        root.profile(ProfilePolicy::ControlledHelper, &reads, Some(port))
            .unwrap(),
        root.profile(ProfilePolicy::VendorNoFork, &reads, Some(port))
            .unwrap(),
    )
}

fn select_profile(mode: MeasurementMode, base: &str) -> String {
    if mode == MeasurementMode::FullFixture {
        return base.to_owned();
    }
    assert!(
        !base.contains("sysctl-read"),
        "unchanged base lacks sysctl exceptions"
    );
    format!("{base}{OS_BANNER_RULE}")
}
fn delta(mode: MeasurementMode) -> &'static str {
    if mode == MeasurementMode::VersionOnlySysctl {
        OS_BANNER_RULE.trim()
    } else {
        "none"
    }
}
fn hash(profile: &str) -> String {
    hf_storage::Sha256Digest
        .digest_parts(&[profile.as_bytes()])
        .as_str()
        .to_owned()
}
