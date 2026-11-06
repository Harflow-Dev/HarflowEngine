#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    capture::FileIdentity,
    fixture::QualifiedLedger,
    owner_policy::{
        AuthorityContext, AuthorityRole, FixtureAssignment, PermissionMode, PreparationEnvelope,
        Requirement, RuntimePolicy,
    },
    ownership::{Digest, Id, PreparedRequest},
    prepared_launch::{BinaryMetadata, BinaryPin, LaunchAssignment, PreparedLaunch},
    process::ProcessIdentity,
    recovery::SourceStamp,
};
use hf_interfaces::{owner_preparation, prepared_launch};

struct TestDigest;
impl DigestPort for TestDigest {
    fn digest_parts(&self, parts: &[&[u8]]) -> Digest {
        let mut output = [0_u8; 32];
        let mut index = 0_usize;
        for part in parts {
            for byte in *part {
                output[index] = output[index].wrapping_mul(31).wrapping_add(*byte);
                index = (index + 13) % output.len();
            }
        }
        Digest::from_sha256_bytes(output)
    }
}

fn id(c: char) -> Id {
    Id::new(c.to_string().repeat(32)).unwrap()
}
fn digest(c: char) -> Digest {
    Digest::new(c.to_string().repeat(64)).unwrap()
}
fn process() -> ProcessIdentity {
    ProcessIdentity::new(41, 100, 4, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
fn policy() -> PreparationEnvelope {
    PreparationEnvelope {
        profile_digest: Digest::new(owner_preparation::PROFILE_DIGEST.into()).unwrap(),
        configuration_digest: digest('a'),
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
            caller: process(),
            role: AuthorityRole::Direct,
            checkpoint: None,
            revision: None,
            issued_attempt: id('4'),
            grant_digest: digest('b'),
        },
        assignment: FixtureAssignment::FixedEffect,
    }
}
fn identity(inode: u64) -> FileIdentity {
    FileIdentity { device: 1, inode }
}
fn prepared() -> PreparedLaunch {
    let source = SourceStamp {
        generation: id('5'),
        root: identity(10),
        database: identity(11),
        marker: identity(12),
        store_lock: identity(13),
        io_lock: identity(14),
    };
    let mut value = PreparedLaunch {
        policy: policy(),
        assignment: LaunchAssignment::FixedProtocolHelper,
        binary: BinaryPin {
            logical_path: "/private/tmp/helper".into(),
            physical_path: "/private/tmp/helper".into(),
            metadata: BinaryMetadata {
                identity: identity(20),
                length: 1234,
                modified_seconds: 1_700_000_000,
                modified_nanoseconds: 1,
                changed_seconds: 1_700_000_000,
                changed_nanoseconds: 2,
            },
            digest: digest('c'),
        },
        argv: vec!["--fixture-fixed-protocol-helper".into()],
        environment: Vec::new(),
        cwd: "/private/tmp".into(),
        source,
        ledger: QualifiedLedger {
            device: 1,
            inode: 21,
        },
        source_profile: "fixed-protocol-helper-v1".into(),
    };
    value.policy.configuration_digest =
        prepared_launch::configuration_digest(&value, &TestDigest).unwrap();
    value
}
fn request(payload: Vec<u8>) -> PreparedRequest {
    PreparedRequest {
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: id('5'),
        digest: TestDigest.digest_parts(&[&payload]),
        payload,
    }
}

#[test]
fn fixed_helper_codec_round_trips_and_legacy_hfop_bytes_stay_unchanged() {
    let launch = prepared();
    let encoded = prepared_launch::encode(&launch, &TestDigest).unwrap();
    assert!(encoded.starts_with(b"HFPL\x01"));
    assert_eq!(
        prepared_launch::decode(&request(encoded.clone()), &TestDigest).unwrap(),
        launch
    );
    assert!(matches!(
        prepared_launch::decode_preparation(&request(encoded), &TestDigest).unwrap(),
        prepared_launch::DecodedPreparation::FixedProtocol(_)
    ));

    let legacy = owner_preparation::encode(&policy()).unwrap();
    assert_eq!(&legacy[..5], b"HFOP\x01");
    assert_eq!(legacy.len(), 459);
    assert_eq!(owner_preparation::encode(&policy()).unwrap(), legacy);
    assert!(matches!(
        prepared_launch::decode_preparation(&request(legacy), &TestDigest).unwrap(),
        prepared_launch::DecodedPreparation::Legacy(_)
    ));
}

#[test]
fn hfpl_truncation_trailing_unknown_assignment_and_size_refuse() {
    let encoded = prepared_launch::encode(&prepared(), &TestDigest).unwrap();
    for bytes in [
        &encoded[..encoded.len() - 1],
        &[encoded.as_slice(), &[0]].concat(),
    ] {
        assert!(prepared_launch::decode(&request(bytes.to_vec()), &TestDigest).is_err());
    }
    let mut unknown = encoded.clone();
    let suffix = 5 + 4 + 459;
    unknown[suffix] = 1;
    assert!(prepared_launch::decode(&request(unknown), &TestDigest).is_err());
    assert!(prepared_launch::decode(&request(vec![b'x'; 16_385]), &TestDigest).is_err());
}

#[test]
fn fixed_helper_configuration_and_source_identity_are_bound() {
    let value = prepared();
    let encoded = prepared_launch::encode(&value, &TestDigest).unwrap();
    let mut changed = value.clone();
    changed.binary.digest = digest('d');
    assert!(prepared_launch::encode(&changed, &TestDigest).is_err());
    changed = value;
    changed.source.database = identity(99);
    assert!(prepared_launch::encode(&changed, &TestDigest).is_err());
    assert_eq!(
        prepared_launch::decode(&request(encoded), &TestDigest).unwrap(),
        prepared()
    );
}
