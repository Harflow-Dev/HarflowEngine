#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::owner_runtime::DigestPort;
use hf_domain::{
    capture::FileIdentity,
    owner_policy::{
        AuthorityContext, AuthorityRole, FixtureAssignment, PermissionMode, PreparationEnvelope,
        Requirement, RuntimePolicy,
    },
    owner_runtime::StorageParticipant,
    ownership::{Digest, Id, OwnerClaim, PreparedRequest},
    process::ProcessIdentity,
    recovery::SourceStamp,
};
use hf_interfaces::owner_preparation::{
    MAX_PAYLOAD, PROFILE_DIGEST, PreparationError, decode, encode, validate,
};

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
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 100, 4, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
fn envelope() -> PreparationEnvelope {
    PreparationEnvelope {
        profile_digest: Digest::new(PROFILE_DIGEST.into()).unwrap(),
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
            caller: process(41),
            role: AuthorityRole::Direct,
            checkpoint: None,
            revision: None,
            issued_attempt: id('4'),
            grant_digest: digest('b'),
        },
        assignment: FixtureAssignment::FixedEffect,
    }
}
fn request(value: &PreparationEnvelope) -> PreparedRequest {
    let payload = encode(value).unwrap();
    PreparedRequest {
        operation: value.operation.clone(),
        session: value.session.clone(),
        reservation: value.reservation.clone(),
        attempt: value.attempt.clone(),
        generation: value.generation.clone(),
        digest: TestDigest.digest_parts(&[&payload]),
        payload,
    }
}
fn participant(value: &PreparationEnvelope, request: &PreparedRequest) -> StorageParticipant {
    let file = FileIdentity {
        device: 1,
        inode: 2,
    };
    StorageParticipant {
        source: SourceStamp {
            generation: value.generation.clone(),
            root: file.clone(),
            database: file.clone(),
            marker: file.clone(),
            store_lock: file.clone(),
            io_lock: file,
        },
        operation: request.operation.clone(),
        attempt: request.attempt.clone(),
        request_digest: request.digest.clone(),
        configuration_digest: value.configuration_digest.clone(),
        profile_digest: value.profile_digest.clone(),
        owner: OwnerClaim {
            owner: id('6'),
            nonce: id('7'),
            process: process(42),
        },
        writer: process(43),
        channel_nonce: id('8'),
    }
}

#[test]
fn canonical_preparation_round_trip_binds_request_digest_and_outer_ids() {
    let value = envelope();
    let request = request(&value);
    assert_eq!(request.payload.len(), 459);
    assert!(request.payload.len() <= MAX_PAYLOAD);
    assert_eq!(decode(&request, &TestDigest).unwrap(), value);

    let mut wrong_digest = request.clone();
    wrong_digest.digest = digest('c');
    assert_eq!(
        decode(&wrong_digest, &TestDigest),
        Err(PreparationError::Binding)
    );
    let mut wrong_outer = request.clone();
    wrong_outer.operation = id('9');
    wrong_outer.digest = TestDigest.digest_parts(&[&wrong_outer.payload]);
    assert_eq!(
        decode(&wrong_outer, &TestDigest),
        Err(PreparationError::Binding)
    );
}

#[test]
fn participant_validation_binds_configuration_profile_operation_and_source_generation() {
    let value = envelope();
    let request = request(&value);
    let participant = participant(&value, &request);
    assert_eq!(
        validate(&request, &participant, &TestDigest).unwrap(),
        value
    );

    let mut mismatch = participant.clone();
    mismatch.configuration_digest = digest('c');
    assert_eq!(
        validate(&request, &mismatch, &TestDigest),
        Err(PreparationError::Binding)
    );
    let mut mismatch = participant.clone();
    mismatch.profile_digest = digest('d');
    assert_eq!(
        validate(&request, &mismatch, &TestDigest),
        Err(PreparationError::Binding)
    );
    let mut mismatch = participant.clone();
    mismatch.source.generation = id('9');
    assert_eq!(
        validate(&request, &mismatch, &TestDigest),
        Err(PreparationError::Binding)
    );
    let mut mismatch = participant;
    mismatch.operation = id('9');
    assert_eq!(
        validate(&request, &mismatch, &TestDigest),
        Err(PreparationError::Binding)
    );
}

#[test]
fn malformed_unknown_enums_boolean_tags_and_trailing_bytes_refuse() {
    let request = request(&envelope());
    let mut malformed = request.clone();
    malformed.payload.truncate(malformed.payload.len() - 1);
    malformed.digest = TestDigest.digest_parts(&[&malformed.payload]);
    assert_eq!(
        decode(&malformed, &TestDigest),
        Err(PreparationError::Shape)
    );

    let mut malformed = request.clone();
    malformed.payload.push(0);
    malformed.digest = TestDigest.digest_parts(&[&malformed.payload]);
    assert_eq!(
        decode(&malformed, &TestDigest),
        Err(PreparationError::Shape)
    );

    for (offset, value, expected) in [
        (293, 2, PreparationError::Policy),
        (294, 2, PreparationError::Policy),
        (303, 2, PreparationError::Shape),
        (360, 9, PreparationError::Shape),
    ] {
        let mut malformed = request.clone();
        malformed.payload[offset] = value;
        malformed.digest = TestDigest.digest_parts(&[&malformed.payload]);
        assert_eq!(
            decode(&malformed, &TestDigest),
            Err(expected),
            "offset {offset}"
        );
    }
}

#[test]
fn invalid_policy_and_authority_scope_cannot_be_encoded_as_static_preparation() {
    let mut invalid = envelope();
    invalid.policy.permission_mode = PermissionMode::ToolLevel;
    assert_eq!(encode(&invalid), Err(PreparationError::Policy));
    let mut invalid = envelope();
    invalid.policy.background_idle_seconds = f64::NAN;
    assert_eq!(encode(&invalid), Err(PreparationError::Policy));
    let mut invalid = envelope();
    invalid.authority.issued_attempt = id('9');
    assert_eq!(encode(&invalid), Err(PreparationError::Binding));
    let mut invalid = envelope();
    invalid.authority.role = AuthorityRole::Worker;
    assert_eq!(encode(&invalid), Err(PreparationError::Binding));
    let mut valid_worker = envelope();
    valid_worker.authority.role = AuthorityRole::Worker;
    valid_worker.authority.checkpoint = Some(id('a'));
    valid_worker.authority.revision = Some(7);
    assert!(encode(&valid_worker).is_ok());
}
