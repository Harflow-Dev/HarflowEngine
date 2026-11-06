#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::Measurement;
#[path = "owner/pipeline.rs"]
mod pipeline;
use crate::claude_measurement::{identity, invocation, server, server::ServerOutcome};
use hf_application::{
    owner_runtime::DigestPort,
    prepared_launch::{CaptureContext, ExactCaptureReader, ReceiptDeliveryPort},
};
use hf_domain::{
    claude_qualification::{
        ClaudeQualification, QualificationCommand, VENDOR_DIGEST, VENDOR_LENGTH, VENDOR_PATH,
        VENDOR_VERSION,
    },
    fixture::{FixtureFault, Frame, ParentPlan, QualifiedLedger},
    owner_policy::{
        AuthorityContext, AuthorityRole, FixtureAssignment, PermissionMode, PreparationEnvelope,
        Requirement, RuntimePolicy,
    },
    ownership::{Digest, Id, PreparedRequest},
    process::ProcessExit,
};
use hf_interfaces::{claude_qualification, fixture};
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnInitialization, SpawnSpec, WaitOutcome},
    qualification::recorded_vendor_profile,
};
use hf_storage::{Limits, RecoveryReadSession, RootHold, Sha256Digest, Store};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const PREFLIGHT_SECONDS: u64 = 5;
const VERSION_SECONDS: u64 = 15;
const ICU_PATH: &str = "/usr/share/icu/icudt78l.dat";
const ICU_LENGTH: u64 = 35_444_880;
const ICU_SHA256: &str = "666a97d4dd1139f537290d74a661cc6b6da6cc58da24486b3cb282eacbcfa549";

fn id(ch: char) -> Id {
    Id::new(ch.to_string().repeat(32)).unwrap()
}
fn digest(ch: char) -> hf_domain::ownership::Digest {
    hf_domain::ownership::Digest::new(ch.to_string().repeat(64)).unwrap()
}
fn binary() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("harflow-engine-scaffold")
}

struct CurrentDirectory(PathBuf);
impl CurrentDirectory {
    fn enter(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(previous)
    }
}
impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

struct Delivery(Option<hf_domain::prepared_launch::CaptureAccepted>);
impl ReceiptDeliveryPort for Delivery {
    fn receive(
        &self,
        invitation: &hf_domain::fixture::Invitation,
        _deadline: Instant,
    ) -> Result<
        Option<hf_domain::prepared_launch::CaptureAccepted>,
        hf_application::recovery::RecoveryError,
    > {
        Ok(self
            .0
            .as_ref()
            .filter(|receipt| &receipt.invitation == invitation)
            .cloned())
    }
}

pub(super) fn run(mut measurement: Measurement, overall_until: Instant) {
    let control_before = measurement
        .owner_control
        .as_deref()
        .map(control_snapshot)
        .transpose()
        .expect("owner control sentinel baseline");
    let attempt = catch_unwind(AssertUnwindSafe(|| {
        attempt_owner_protocol(&mut measurement, overall_until, control_before.as_ref())
    }));
    let _ = measurement.server.release_post();
    let ipv6_quiet = measurement.server.no_ipv6_connection();
    let denied_port_quiet = measurement.denied_listener.no_connection();
    eprintln!(
        "phase=qualification-settlement ipv6-quiet={ipv6_quiet} denied-port-quiet={denied_port_quiet}"
    );
    let server_outcome = measurement.server.stop_and_join_result();
    eprintln!("phase=qualification-settlement server-outcome={server_outcome:?}");
    let canary_result = verify_canary(&measurement.canary, &measurement.canary_identity);
    let control_result = control_before
        .as_ref()
        .map(verify_control_snapshot)
        .unwrap_or(Ok(()));
    let mut failures = Vec::new();
    match attempt {
        Ok(Ok(observation)) => {
            if let Err(error) = catch_unwind(AssertUnwindSafe(|| {
                observation.require_protocol_witnesses(&measurement.nonce)
            })) {
                failures.push(format!("strict protocol oracle: {}", panic_message(error)));
            }
        }
        Ok(Err(error)) => failures.push(format!("owner pipeline: {error}")),
        Err(error) => failures.push(format!("owner phase panicked: {}", panic_message(error))),
    }
    if !ipv6_quiet {
        failures.push("IPv6 listener was not quiet".into());
    }
    if !denied_port_quiet {
        failures.push("unlisted listener was not quiet".into());
    }
    if server_outcome != Ok(ServerOutcome::Completed) {
        failures.push(format!("server did not complete: {server_outcome:?}"));
    }
    if let Err(error) = canary_result {
        failures.push(error);
    }
    if let Err(error) = control_result {
        failures.push(error);
    }
    if !failures.is_empty() {
        panic!(
            "owner-hosted qualification refused after settlement witnesses: {}",
            failures.join("; ")
        );
    }
    canary_cleanup(&measurement.canary, &measurement.canary_identity);
    measurement.owned.settle();
    eprintln!("phase=owner-hosted-qualification settled; production remains unsupported");
}

fn attempt_owner_protocol(
    measurement: &mut Measurement,
    overall_until: Instant,
    control_before: Option<&ControlSnapshot>,
) -> Result<pipeline::Observation, String> {
    let preflight_until =
        overall_until.min(Instant::now() + Duration::from_secs(PREFLIGHT_SECONDS));
    measurement.prove_preflight_until(preflight_until);
    verify_canary(&measurement.canary, &measurement.canary_identity)?;
    if let Some(snapshot) = control_before {
        verify_control_snapshot(snapshot)?;
    }
    let root = &measurement.owned.fixture;
    let version_until = overall_until.min(Instant::now() + Duration::from_secs(VERSION_SECONDS));
    let platform = platform_observation();
    let icu_before = observe_icu(version_until).expect("approved ICU data observation");
    let vendor_before = hf_platform::prepared_launch::observe_qualification_vendor(
        Path::new(VENDOR_PATH),
        &Sha256Digest,
        version_until,
    )
    .expect("physical vendor pin before version phase");
    if vendor_before.logical_path != VENDOR_PATH
        || vendor_before.physical_path != VENDOR_PATH
        || vendor_before.metadata.length != VENDOR_LENGTH
        || vendor_before.digest.as_str() != VENDOR_DIGEST
    {
        return Err("selected physical Claude binary differs from the approved 2.1.318 pin".into());
    }
    let version = invocation::capture_version_only_until(
        root,
        &measurement.vendor_profile,
        &measurement.selection,
        measurement.server.port(),
        &measurement.nonce,
        version_until,
    );
    let vendor_after = hf_platform::prepared_launch::observe_qualification_vendor(
        Path::new(VENDOR_PATH),
        &Sha256Digest,
        version_until,
    )
    .expect("physical vendor pin after version phase");
    let icu_after = observe_icu(version_until).expect("approved ICU data bookend");
    assert_eq!(
        vendor_before, vendor_after,
        "vendor pin stable across version phase"
    );
    assert!(
        icu_before.same_metadata(&icu_after) && icu_before.sha256 == icu_after.sha256,
        "ICU pin stable across version phase"
    );
    version.require_supported();
    eprintln!(
        "phase=qualification-version exact-version={VENDOR_VERSION} physical-target={VENDOR_PATH} preflight-complete-before-owner=true platform={platform} icu-path={ICU_PATH} icu-length={} icu-sha256={} timezone-data-difference=recorded-only",
        icu_after.length, icu_after.sha256
    );

    let result = pipeline::run_owner_pipeline(measurement, overall_until, &vendor_before)?;
    let vendor_final = hf_platform::prepared_launch::observe_qualification_vendor(
        Path::new(VENDOR_PATH),
        &Sha256Digest,
        overall_until,
    )
    .map_err(show)?;
    let icu_final = observe_icu(overall_until)?;
    if vendor_final != vendor_before {
        return Err("vendor pin changed after owner-hosted protocol".into());
    }
    if !icu_before.same_metadata(&icu_final) || icu_before.sha256 != icu_final.sha256 {
        return Err("ICU pin changed after owner-hosted protocol".into());
    }
    verify_canary(&measurement.canary, &measurement.canary_identity)?;
    if let Some(snapshot) = control_before {
        verify_control_snapshot(snapshot)?;
    }
    Ok(result)
}

fn canary_cleanup(path: &Path, metadata: &fs::Metadata) {
    let current = fs::symlink_metadata(path).expect("outside read canary remains");
    assert_eq!(
        (current.dev(), current.ino(), current.len()),
        (metadata.dev(), metadata.ino(), metadata.len())
    );
    fs::remove_file(path).expect("remove original canary only after full settlement");
}
struct ControlSnapshot {
    path: PathBuf,
    metadata: fs::Metadata,
    bytes: Vec<u8>,
}
fn control_snapshot(path: &Path) -> Result<ControlSnapshot, String> {
    Ok(ControlSnapshot {
        path: path.to_path_buf(),
        metadata: fs::symlink_metadata(path).map_err(show)?,
        bytes: fs::read(path).map_err(show)?,
    })
}
fn verify_control_snapshot(snapshot: &ControlSnapshot) -> Result<(), String> {
    let current = fs::symlink_metadata(&snapshot.path).map_err(show)?;
    let bytes = fs::read(&snapshot.path).map_err(show)?;
    if !identity::same_metadata(&snapshot.metadata, &current) || bytes != snapshot.bytes {
        return Err("owner-control sentinel identity or bytes changed".into());
    }
    Ok(())
}
fn verify_canary(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    let current = fs::symlink_metadata(path).map_err(show)?;
    let bytes = fs::read(path).map_err(show)?;
    if !identity::same_metadata(metadata, &current) || bytes != b"outside-user-data" {
        return Err("outside read canary identity or bytes changed".into());
    }
    Ok(())
}
fn panic_message(error: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = error.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = error.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "non-string panic payload".into()
    }
}
struct IcuObservation {
    metadata: fs::Metadata,
    length: u64,
    sha256: String,
}
impl IcuObservation {
    fn same_metadata(&self, other: &Self) -> bool {
        identity::same_metadata(&self.metadata, &other.metadata)
    }
}

fn observe_icu(until: Instant) -> Result<IcuObservation, String> {
    let path = Path::new(ICU_PATH);
    let before = fs::symlink_metadata(path).map_err(show)?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || before.nlink() != 1
        || before.len() != ICU_LENGTH
        || fs::canonicalize(path).map_err(show)? != path
    {
        return Err("approved ICU data file identity or size changed".into());
    }
    let mut file = fs::File::open(path).map_err(show)?;
    let mut digest = hf_storage::Sha256Stream::with_limit(128 * 1024 * 1024).map_err(show)?;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        if Instant::now() >= until {
            return Err("ICU data observation deadline expired".into());
        }
        let count = file.read(&mut buffer).map_err(show)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]).map_err(show)?;
    }
    let sha256 = digest.finish().map_err(show)?.as_str().to_owned();
    let after = fs::symlink_metadata(path).map_err(show)?;
    if !identity::same_metadata(&before, &after) || sha256 != ICU_SHA256 {
        return Err("approved ICU data bytes or metadata changed".into());
    }
    Ok(IcuObservation {
        metadata: after,
        length: before.len(),
        sha256,
    })
}

fn platform_observation() -> String {
    assert_eq!(std::env::consts::OS, "macos", "macOS is required");
    assert_eq!(std::env::consts::ARCH, "aarch64", "arm64 is required");
    let output = Command::new("/usr/bin/sw_vers")
        .arg("-buildVersion")
        .output()
        .expect("read macOS build identity");
    assert!(output.status.success(), "sw_vers succeeded");
    let build = String::from_utf8(output.stdout)
        .expect("build version is UTF-8")
        .trim()
        .to_owned();
    assert!(
        !build.is_empty() && build.len() <= 32,
        "bounded macOS build identity"
    );
    format!("macOS-build={build} arch=arm64")
}

fn show(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}
