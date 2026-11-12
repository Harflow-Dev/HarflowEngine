#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! A FIFO in place of the acceptance record is `Incompatible` and is never opened.
//! This is the only test in its binary: creating the FIFO spawns `mkfifo`, and a
//! child spawned while sibling tests hold lease descriptors can briefly inherit them.
mod common;
mod read_session_support;
mod wal_acceptance_support;
use hf_storage::ReadRefusal;
use read_session_support::seeded;
use wal_acceptance_support::{RECORD, assert_refused_everywhere, is_incompatible, make_fifo};

#[test]
fn a_fifo_is_incompatible_without_blocking() {
    let seeded = seeded(3);
    assert_refused_everywhere(
        &seeded.root,
        |root| make_fifo(&root.join(RECORD)),
        is_incompatible,
        &ReadRefusal::InvalidMarker,
        "fifo",
    );
}
