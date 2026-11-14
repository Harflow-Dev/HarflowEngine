#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Readers racing commits: every read taken while a writer publishes watermark slots
//! succeeds. A reader reads the watermark before its snapshot, so a commit landing in
//! between leaves the file behind the snapshot, never ahead of it.
mod common;
mod watermark_support;
use common::deadline;
use hf_storage::Error;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use watermark_support::{Recorder, database_seq, initialized, put, session};

const COMMITS: u64 = 300;

#[test]
fn readers_racing_a_writer_see_no_lost_commit_and_no_unavailable_watermark() {
    let (root, store) = initialized(Arc::new(Recorder::default()));
    let root = root.root.clone();
    let done = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let root = root.clone();
            let done = done.clone();
            std::thread::spawn(move || {
                let mut reads = 0u32;
                let mut errors: Vec<String> = Vec::new();
                let long = session(&root).unwrap();
                while !done.load(Ordering::SeqCst) || reads == 0 {
                    match session(&root).and_then(|fresh| fresh.snapshot(deadline(), |_| Ok(()))) {
                        Ok(()) => {}
                        Err(error) => errors.push(format!("open: {error:?}")),
                    }
                    if let Err(error) = long.snapshot(deadline(), |_| Ok(())) {
                        errors.push(format!("long: {error:?}"));
                    }
                    reads += 1;
                }
                (reads, errors)
            })
        })
        .collect();
    for number in 0..COMMITS {
        put(&store, &format!("key-{number}")).unwrap();
    }
    done.store(true, Ordering::SeqCst);
    for reader in readers {
        let (reads, errors) = reader.join().unwrap();
        assert!(reads > 0);
        assert!(
            errors.is_empty(),
            "{} failed reads: {:?}",
            errors.len(),
            &errors[..errors.len().min(5)]
        );
    }
    assert_eq!(database_seq(&root), COMMITS);
    assert!(!matches!(session(&root).err(), Some(Error::CommitLost)));
}
