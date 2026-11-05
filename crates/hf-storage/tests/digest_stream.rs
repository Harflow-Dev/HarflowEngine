#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_storage::{DigestSizeLimit, Sha256Stream};

#[test]
fn incremental_digest_matches_known_sha256_across_chunks() {
    let mut digest = Sha256Stream::with_limit(3).unwrap();
    digest.update(b"a").unwrap();
    digest.update(b"b").unwrap();
    digest.update(b"c").unwrap();
    assert_eq!(
        digest.finish().unwrap().as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn exact_limit_is_accepted_and_overflow_permanently_refuses_finish() {
    let mut exact = Sha256Stream::with_limit(2).unwrap();
    exact.update(b"ok").unwrap();
    assert!(exact.finish().is_ok());

    let mut over = Sha256Stream::with_limit(1).unwrap();
    assert_eq!(over.update(b"xx"), Err(DigestSizeLimit));
    assert_eq!(over.update(b""), Err(DigestSizeLimit));
    assert_eq!(over.finish(), Err(DigestSizeLimit));
}

#[test]
fn stream_budget_has_a_hard_ceiling() {
    assert!(matches!(
        Sha256Stream::with_limit(512 * 1024 * 1024 + 1),
        Err(DigestSizeLimit)
    ));
}
