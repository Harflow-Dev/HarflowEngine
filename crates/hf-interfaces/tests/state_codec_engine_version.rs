#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::ctl::{state_inspect::StateCodecPort, state_recover::journal::JOURNAL_BOUND};
use hf_domain::ctl::identity::PRODUCT_VERSION;
use hf_interfaces::ctl::state_codec::JsonStateCodec;

#[test]
fn reads_only_bounded_top_level_engine_version() {
    let bytes = format!(
        r#"{{"proposal":{{"engine_version":"wrong"}},"engine_version":"{PRODUCT_VERSION}"}}"#
    );
    assert_eq!(
        JsonStateCodec.journal_engine_version(bytes.as_bytes()),
        Some(PRODUCT_VERSION.into())
    );
}

#[test]
fn duplicate_or_missing_top_level_engine_version_is_unqualified() {
    let duplicate = format!(r#"{{"engine_version":"{PRODUCT_VERSION}","engine_version":"other"}}"#);
    assert_eq!(
        JsonStateCodec.journal_engine_version(duplicate.as_bytes()),
        None
    );
    assert_eq!(
        JsonStateCodec.journal_engine_version(br#"{"phase":"completed"}"#),
        None
    );
}

#[test]
fn over_bound_journal_is_not_scanned_for_engine_provenance() {
    let bytes = format!(r#"{{"engine_version":"{PRODUCT_VERSION}"}}"#);
    let mut oversized = vec![b' '; JOURNAL_BOUND + 1];
    oversized[..bytes.len()].copy_from_slice(bytes.as_bytes());
    assert_eq!(JsonStateCodec.journal_engine_version(&oversized), None);
}
