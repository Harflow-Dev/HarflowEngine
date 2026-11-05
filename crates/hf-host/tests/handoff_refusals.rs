#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[path = "handoff_refusals/support.rs"]
mod support;

use hf_domain::fixture::FixtureFault;
use support::{
    BindingFault, EntryFault, MalformedInput, Seed, assert_unchanged, entry_fault,
    finish_entry_refusal, malformed_refuses, mismatched_refuses,
};

#[test]
fn owner_rejects_wrong_nonce_payload_generation_revision_and_physical_owner_without_claim() {
    let seed = Seed::new();
    let before = seed.snapshot.clone();
    for fault in [
        BindingFault::PreparedNonce,
        BindingFault::ChangedPayload,
        BindingFault::ChangedGeneration,
        BindingFault::StaleRevision,
        BindingFault::WrongPhysicalOwner,
    ] {
        mismatched_refuses(&seed, fault);
        assert_unchanged(&seed, &before);
    }
}

#[test]
fn owner_rejects_truncated_malformed_oversized_trailing_and_eof_requests_without_claim() {
    let seed = Seed::new();
    let before = seed.snapshot.clone();
    for fault in [
        MalformedInput::Truncated,
        MalformedInput::InvalidMagic,
        MalformedInput::Oversized,
        MalformedInput::Trailing,
        MalformedInput::Eof,
    ] {
        malformed_refuses(&seed, fault);
        assert_unchanged(&seed, &before);
    }
}

#[test]
fn seeded_fixture_is_prepared_only_before_owner_handoff() {
    let seed = Seed::new();
    assert!(seed.snapshot.contains("\"phase\": 0"));
    assert!(seed.snapshot.contains("\"revision\": 0"));
    assert!(seed.snapshot.contains("\"generation\": \""));
    assert!(matches!(seed.fault, FixtureFault::None));
}

#[test]
fn entry_refuses_environment_extra_descriptor_and_nonpipe_stdin_without_store_changes() {
    let seed = Seed::new();
    let before = seed.snapshot.clone();
    for fault in [
        EntryFault::Environment,
        EntryFault::ExtraDescriptor,
        EntryFault::RegularStdin,
    ] {
        let mut child = entry_fault(&seed, fault);
        finish_entry_refusal(&mut child);
        assert_unchanged(&seed, &before);
    }
}
