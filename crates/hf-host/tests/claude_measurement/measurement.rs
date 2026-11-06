#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_platform::qualification::{FixtureRoot, ProfilePolicy};
use std::{
    fs::{self, Metadata, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

use super::{SettledRoot, canary, identity, invocation, server};

#[path = "measurement/profile.rs"]
mod profile;
#[path = "measurement/settlement.rs"]
mod settlement;

pub(super) const PYTHON: &str = "/Library/Developer/CommandLineTools/usr/bin/python3";
const PYTHON_RUNTIME: &str =
    "/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework";
pub(super) const HELPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/claude_measurement/preflight.py"
);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MeasurementMode {
    FullFixture,
    VersionOnlySysctl,
    PageSizeVersionOnly,
    CompatPageSizeVersionOnly,
    CompatProtocol,
    OwnerQualification,
}

struct Measurement {
    selection: identity::VendorSelection,
    owned: SettledRoot,
    nonce: String,
    canary: PathBuf,
    canary_identity: Metadata,
    owner_control: Option<PathBuf>,
    positive: PathBuf,
    server: server::FixtureServer,
    denied_listener: server::DeniedListener,
    controlled_profile: String,
    vendor_profile: String,
}

pub(super) fn run(mode: MeasurementMode) {
    let overall_until = (mode == MeasurementMode::OwnerQualification)
        .then(|| Instant::now() + Duration::from_secs(130));
    let measurement = Measurement::create(mode);
    if mode == MeasurementMode::OwnerQualification {
        owner::run(
            measurement,
            overall_until.expect("owner qualification deadline"),
        );
        return;
    }
    measurement.prove_preflight();
    if mode == MeasurementMode::CompatProtocol {
        let observation = invocation::capture_protocol(
            &measurement.owned.fixture,
            &measurement.vendor_profile,
            &measurement.selection,
            measurement.server.port(),
            &measurement.nonce,
        );
        measurement.finish_protocol(observation);
    } else if candidate_only(mode) {
        let observation = invocation::capture_version_only(
            &measurement.owned.fixture,
            &measurement.vendor_profile,
            &measurement.selection,
            measurement.server.port(),
            &measurement.nonce,
        );
        measurement.finish_candidate(observation);
    } else {
        measurement.invoke(mode);
        measurement.finish(mode);
    }
}

#[path = "measurement/owner.rs"]
mod owner;

impl Measurement {
    fn create(mode: MeasurementMode) -> Self {
        let selection = if mode == MeasurementMode::OwnerQualification {
            identity::qualification_selection()
        } else {
            identity::selection()
        };
        let owned = SettledRoot::create();
        let root = &owned.fixture;
        let nonce = identity::nonce_from_root(root.root());
        let (canary, canary_identity, positive) = canaries(root, &nonce);
        let owner_control = if mode == MeasurementMode::OwnerQualification {
            let path = root.root().join("owner-control-sentinel");
            fs::write(&path, b"owner state is outside vendor write scope").unwrap();
            Some(path)
        } else {
            None
        };
        invocation::prepare_private_dirs(root);
        let server = if mode == MeasurementMode::OwnerQualification {
            server::FixtureServer::start_owner_protocol(nonce.clone())
        } else if candidate_only(mode) {
            server::FixtureServer::start_preflight_only(nonce.clone())
        } else {
            server::FixtureServer::start(nonce.clone())
        }
        .expect("reserve same-port IPv4 and IPv6 loopback listeners");
        let denied_listener = server::DeniedListener::bind(server.port()).unwrap();
        let (controlled_profile, base_vendor) = profiles(root, &selection, server.port());
        let vendor_profile = if mode == MeasurementMode::OwnerQualification {
            let topology = root.recorded_topology().expect("recorded fixture topology");
            hf_platform::qualification::recorded_vendor_profile(&topology, server.port())
                .expect("exact recorded Claude profile")
        } else {
            profile::select(mode, &base_vendor)
        };
        eprintln!(
            "phase=profile-created mode={mode:?} fixture-root={} fixture-port={} root-identity={:?} base-profile-sha256={} selected-profile-sha256={} diagnostic-delta={}",
            root.root().display(),
            server.port(),
            root.identity(),
            profile::hash(&base_vendor),
            profile::hash(&vendor_profile),
            profile::delta(mode)
        );
        Self {
            selection,
            owned,
            nonce,
            canary,
            canary_identity,
            owner_control,
            positive,
            server,
            denied_listener,
            controlled_profile,
            vendor_profile,
        }
    }

    fn prove_preflight(&self) {
        self.prove_preflight_with_deadline(None);
    }

    fn prove_preflight_until(&self, until: Instant) {
        self.prove_preflight_with_deadline(Some(until));
    }

    fn prove_preflight_with_deadline(&self, until: Option<Instant>) {
        let root = &self.owned.fixture;
        eprintln!("phase=controlled-posix-spawn-positive begin");
        assert_eq!(
            super::launch_spawn_control(root, &self.controlled_profile, until),
            "spawn-control=allowed"
        );
        eprintln!("phase=controlled-posix-spawn-positive complete");
        eprintln!("phase=vendor-nofork-read-network-preflight begin");
        let result = super::launch_preflight(
            root,
            &self.vendor_profile,
            &self.canary,
            &self.positive,
            super::PreflightContext {
                allowed_port: self.server.port(),
                denied_port: self.denied_listener.port(),
                nonce: &self.nonce,
                owner_control: self.owner_control.as_deref(),
                phase_until: until,
            },
        );
        let expected = if self.owner_control.is_some() {
            "fork=1 posix_spawn=1 read=1 inside=1 allowed=1 denied_port=1 owner_write_denied=1"
        } else {
            "fork=1 posix_spawn=1 read=1 inside=1 allowed=1 denied_port=1"
        };
        assert_eq!(result, expected);
        if let Some(until) = until {
            assert!(
                Instant::now() < until,
                "shared five second preflight deadline"
            );
        }
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
            MeasurementMode::PageSizeVersionOnly
            | MeasurementMode::CompatPageSizeVersionOnly
            | MeasurementMode::CompatProtocol => {
                unreachable!("candidate has deferred settlement")
            }
            MeasurementMode::OwnerQualification => {
                unreachable!("owner mode uses its isolated harness")
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
        let expected = match mode {
            MeasurementMode::FullFixture
            | MeasurementMode::CompatProtocol
            | MeasurementMode::OwnerQualification => server::ServerOutcome::Completed,
            MeasurementMode::VersionOnlySysctl
            | MeasurementMode::PageSizeVersionOnly
            | MeasurementMode::CompatPageSizeVersionOnly => server::ServerOutcome::PreflightOnly,
        };
        let Self {
            selection,
            mut owned,
            nonce: _,
            canary,
            canary_identity,
            owner_control: _,
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

fn candidate_only(mode: MeasurementMode) -> bool {
    matches!(
        mode,
        MeasurementMode::PageSizeVersionOnly | MeasurementMode::CompatPageSizeVersionOnly
    )
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
