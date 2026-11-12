#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The launch namespace of the pending reads: revision, invalid names, bounds, binding.
mod common;
mod fixture_support;
mod pending_support;
use fixture_support::{A, B, world};
use hf_application::ctl::task_reads::PendingLimits;
use pending_support::{EMPTY_DIGEST, invalid, launch, namespace, put};

fn sha_of(names: &str) -> &'static str {
    match names {
        "ab-c.launch.json\0ab.launch.json\0" => {
            "6fa032af3390171b0bd3fde862ba4b96597fbbba410cc7d3a771e38bef848462"
        }
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa.launch.json\0" => {
            "0209e82417812caa0622941a47294e7d9caaaecbc231db3bd22d4d2533aa03a7"
        }
        other => panic!("no literal for {other:?}"),
    }
}

#[test]
fn the_empty_namespace_is_the_sha256_of_nothing() {
    let w = world();
    let found = namespace(&w, &PendingLimits::default()).unwrap();
    assert_eq!(found.revision, EMPTY_DIGEST);
    assert_eq!(found.invalid, 0);
}

#[test]
fn one_name_hashes_its_filename_and_a_nul() {
    let w = world();
    put(&w, &launch(A, "reserved"));
    let found = namespace(&w, &PendingLimits::default()).unwrap();
    assert_eq!(
        found.revision,
        sha_of("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa.launch.json\0")
    );
}

#[test]
fn prefix_nested_ids_sort_by_the_full_filename() {
    // `ab-c.launch.json` sorts before `ab.launch.json` because '-' is below '.'.
    let w = world();
    put(&w, &launch("ab", "reserved"));
    put(&w, &launch("ab-c", "reserved"));
    let found = namespace(&w, &PendingLimits::default()).unwrap();
    assert_eq!(found.revision, sha_of("ab-c.launch.json\0ab.launch.json\0"));
}

#[test]
fn invalid_names_are_counted_and_belong_to_the_revision() {
    let w = world();
    put(&w, &launch(A, "reserved"));
    let before = namespace(&w, &PendingLimits::default()).unwrap();
    put(&w, &invalid("not a task id"));
    put(&w, &invalid(".hidden"));
    let after = namespace(&w, &PendingLimits::default()).unwrap();
    assert_eq!((before.invalid, after.invalid), (0, 2));
    assert_ne!(before.revision, after.revision);
}

#[test]
fn a_content_change_keeps_the_revision_and_a_name_change_moves_it() {
    let w = world();
    put(&w, &launch(A, "reserved"));
    let first = namespace(&w, &PendingLimits::default()).unwrap().revision;
    let mut changed = launch(A, "granted");
    changed.title = Some("Another title".into());
    put(&w, &changed);
    let same = namespace(&w, &PendingLimits::default()).unwrap().revision;
    assert_eq!(first, same);
    put(&w, &launch(B, "reserved"));
    let added = namespace(&w, &PendingLimits::default()).unwrap().revision;
    assert_ne!(first, added);
    w.raw()
        .execute("DELETE FROM launch_attempts WHERE name=?1", [B])
        .unwrap();
    let removed = namespace(&w, &PendingLimits::default()).unwrap().revision;
    assert_eq!(first, removed);
}

#[test]
fn the_entry_bound_is_inclusive() {
    let w = world();
    put(&w, &launch(A, "reserved"));
    put(&w, &launch(B, "reserved"));
    let at = PendingLimits {
        entries: 2,
        ..PendingLimits::default()
    };
    let below = PendingLimits {
        entries: 1,
        ..PendingLimits::default()
    };
    assert!(namespace(&w, &at).is_some());
    assert!(namespace(&w, &below).is_none());
}

#[test]
fn the_name_byte_bound_counts_the_suffix_and_is_inclusive() {
    let w = world();
    put(&w, &launch("ab", "reserved"));
    put(&w, &invalid("xyz"));
    // 2 + 3 name bytes plus two ".launch.json" suffixes of 12 bytes.
    let exact = 2 + 3 + 24;
    let at = PendingLimits {
        name_bytes: exact,
        ..PendingLimits::default()
    };
    let below = PendingLimits {
        name_bytes: exact - 1,
        ..PendingLimits::default()
    };
    assert!(namespace(&w, &at).is_some());
    assert!(namespace(&w, &below).is_none());
}

#[test]
fn the_binding_is_deterministic_and_hashes_the_lexical_root() {
    let w = world();
    let bind = |root: &str| {
        w.session()
            .snapshot(fixture_support::later(5), |s| s.pending_binding(root))
            .unwrap()
    };
    let first = bind("/s");
    assert_eq!(
        first.root,
        "8e295f312d7e62d4db0ad6d62af018762334aad2c67fe3479a0b57115ac6c292"
    );
    assert_eq!(first, bind("/s"));
    assert_ne!(first.root, bind("/t").root);
    assert_eq!(first.identity, bind("/t").identity);
}
