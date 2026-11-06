#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    capture::FileIdentity,
    claude_qualification::{
        ClaudeQualification, QualificationCommand, RecordedDirectory, RecordedFixtureTopology,
        VENDOR_DIGEST, VENDOR_LENGTH, VENDOR_PATH, VENDOR_VERSION,
    },
    fixture::QualifiedLedger,
    owner_policy::{
        AuthorityContext, AuthorityRole, FixtureAssignment, PermissionMode, PreparationEnvelope,
        Requirement, RuntimePolicy,
    },
    ownership::{Digest, Id, PreparedRequest},
    prepared_launch::{BinaryMetadata, BinaryPin},
    process::ProcessIdentity,
    recovery::SourceStamp,
};
use hf_interfaces::owner_preparation::PROFILE_DIGEST;
use hf_interfaces::{
    claude_qualification,
    prepared_launch::{DecodedPreparation, decode_preparation},
};

struct TestDigest;
impl DigestPort for TestDigest {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest {
        if parts == [b"(version 1)\n(deny default)\n"] {
            return Digest::new(PROFILE_DIGEST.into()).unwrap();
        }
        let mut state = [0_u8; 32];
        let mut offset = 0_usize;
        for byte in parts.iter().flat_map(|part| part.iter().copied()) {
            state[offset] = state[offset]
                .wrapping_mul(31)
                .wrapping_add(byte)
                .wrapping_add(offset as u8);
            offset = (offset + 1) % state.len();
        }
        Digest::from_sha256_bytes(state)
    }
}
fn id(ch: char) -> Id {
    Id::new(ch.to_string().repeat(32)).unwrap()
}
fn digest(ch: char) -> Digest {
    Digest::new(ch.to_string().repeat(64)).unwrap()
}
fn identity(inode: u64) -> FileIdentity {
    FileIdentity { device: 1, inode }
}
fn directory(path: String, inode: u64) -> RecordedDirectory {
    RecordedDirectory {
        path,
        identity: identity(inode),
    }
}
fn pin(logical: &str, physical: &str, length: u64, sha: &str, inode: u64) -> BinaryPin {
    BinaryPin {
        logical_path: logical.into(),
        physical_path: physical.into(),
        metadata: BinaryMetadata {
            identity: identity(inode),
            length,
            modified_seconds: 1,
            modified_nanoseconds: 2,
            changed_seconds: 3,
            changed_nanoseconds: 4,
        },
        digest: Digest::new(sha.into()).unwrap(),
    }
}
fn value() -> ClaudeQualification {
    let root = "/tmp/hf-qualification".to_owned();
    let profile = b"(version 1)\n(deny default)\n".to_vec();
    let mut v = ClaudeQualification {
        policy: PreparationEnvelope {
            profile_digest: Digest::new(PROFILE_DIGEST.into()).unwrap(),
            configuration_digest: digest('b'),
            operation: id('1'),
            session: id('2'),
            reservation: id('3'),
            attempt: id('4'),
            generation: id('5'),
            policy: RuntimePolicy {
                background_requirement: Requirement::Required,
                permission_mode: PermissionMode::OsEnforced,
                background_idle_seconds: 600.0,
                tool_level_opt_in: false,
            },
            authority: AuthorityContext {
                caller: ProcessIdentity::new(
                    41,
                    100,
                    4,
                    "abcdef01-2345-6789-abcd-ef0123456789".into(),
                )
                .unwrap(),
                role: AuthorityRole::Direct,
                checkpoint: None,
                revision: None,
                issued_attempt: id('4'),
                grant_digest: digest('c'),
            },
            assignment: FixtureAssignment::FixedEffect,
        },
        gate_binary: pin("/tmp/hf-gate", "/tmp/hf-gate", 1024, &"d".repeat(64), 10),
        vendor_binary: pin(VENDOR_PATH, VENDOR_PATH, VENDOR_LENGTH, VENDOR_DIGEST, 11),
        vendor_version: VENDOR_VERSION.into(),
        logical_hint: "/Users/nayanda/.local/bin/claude".into(),
        cwd: root.clone(),
        source: SourceStamp {
            generation: id('5'),
            root: identity(20),
            database: identity(21),
            marker: identity(22),
            store_lock: identity(23),
            io_lock: identity(24),
        },
        ledger: QualifiedLedger {
            device: 1,
            inode: 30,
        },
        topology: RecordedFixtureTopology {
            root: directory(root, 31),
            roles: (0..6)
                .map(|n| directory(format!("/tmp/hf-qualification/role-{n}"), 40 + n))
                .collect(),
        },
        nonce: "0123456789abcdef".into(),
        port: 43210,
        profile: profile.clone(),
        profile_digest: TestDigest.digest_parts(&[&profile]),
        command: QualificationCommand {
            executable: VENDOR_PATH.into(),
            args: vec!["--print".into(), "nonce-0123456789abcdef".into()],
            environment: vec![
                ("ICU_TIMEZONE_FILES_DIR".into(), "".into()),
                ("ANTHROPIC_API_KEY".into(), "fake-qualification-key".into()),
            ],
        },
    };
    v.policy.configuration_digest =
        claude_qualification::configuration_digest(&v, &TestDigest).unwrap();
    v
}
fn request(payload: Vec<u8>) -> PreparedRequest {
    let v = value();
    PreparedRequest {
        operation: v.policy.operation,
        session: v.policy.session,
        reservation: v.policy.reservation,
        attempt: v.policy.attempt,
        generation: v.policy.generation,
        digest: TestDigest.digest_parts(&[&payload]),
        payload,
    }
}

#[test]
fn hfcq_round_trip_is_a_distinct_closed_preparation_route() {
    let v = value();
    let payload = claude_qualification::encode(&v, &TestDigest).unwrap();
    assert!(payload.starts_with(b"HFCQ\x01"));
    assert_eq!(
        claude_qualification::decode(&request(payload.clone()), &TestDigest).unwrap(),
        v
    );
    assert!(matches!(
        decode_preparation(&request(payload), &TestDigest).unwrap(),
        DecodedPreparation::ClaudeQualification(_)
    ));
}

#[test]
fn envelope_and_inner_configuration_bind_every_encoded_byte() {
    let v = value();
    let mut payload = claude_qualification::encode(&v, &TestDigest).unwrap();
    let offset = payload.len() - 1;
    payload[offset] ^= 1;
    assert!(claude_qualification::decode(&request(payload), &TestDigest).is_err());
    let mut changed = v;
    changed.command.args[1].push('x');
    assert!(claude_qualification::encode(&changed, &TestDigest).is_err());
}

#[test]
fn wrong_vendor_identity_and_unbounded_command_or_topology_refuse() {
    let mut changed = value();
    changed.vendor_version = "2.1.319".into();
    assert!(claude_qualification::encode(&changed, &TestDigest).is_err());
    let mut changed = value();
    changed.vendor_binary.digest = digest('e');
    assert!(claude_qualification::encode(&changed, &TestDigest).is_err());
    let mut changed = value();
    changed.command.args.push("x".repeat(4097));
    assert!(claude_qualification::encode(&changed, &TestDigest).is_err());
    let mut changed = value();
    changed.topology.roles.pop();
    assert!(claude_qualification::encode(&changed, &TestDigest).is_err());
}

#[test]
fn legacy_and_fixed_helper_routes_do_not_accept_hfcq_as_their_payload() {
    let payload = claude_qualification::encode(&value(), &TestDigest).unwrap();
    let mut req = request(payload);
    req.payload[4] = 2;
    req.digest = TestDigest.digest_parts(&[&req.payload]);
    assert!(claude_qualification::decode(&req, &TestDigest).is_err());
    assert!(decode_preparation(&req, &TestDigest).is_err());
}

#[test]
fn truncated_and_unknown_hfcq_versions_fail_closed() {
    for bytes in [
        b"".as_slice(),
        b"H".as_slice(),
        b"HFCQ".as_slice(),
        b"HFCQ\x01".as_slice(),
    ] {
        let req = request(bytes.to_vec());
        assert!(claude_qualification::decode(&req, &TestDigest).is_err());
        assert!(decode_preparation(&req, &TestDigest).is_err());
    }
    let mut req = request(b"HFCQ\x02".to_vec());
    req.digest = TestDigest.digest_parts(&[&req.payload]);
    assert!(claude_qualification::decode(&req, &TestDigest).is_err());
    assert!(decode_preparation(&req, &TestDigest).is_err());
}
