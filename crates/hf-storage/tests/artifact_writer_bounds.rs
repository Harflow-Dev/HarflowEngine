#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `Store::register_artifact` bounds: the 100-item cap, the serialized manifest edge at
//! 1,048,576 bytes, the value bounds checked before the task is looked up, and the fixture
//! replace that bypasses the duplicate, cap and size checks.
mod artifact_writer_support;
mod common;
mod fixture_support;
mod task_writer_support;
use artifact_writer_support::{
    ENVELOPE_A, ITEM_A, MANIFEST_MAX, added, item, items, lengths, observation, plan_item, refused,
    register, usual,
};
use fixture_support::{A, B, C, World, artifact_item, later, one, tid, world};
use hf_domain::ctl::history::ArtifactItem;
use hf_storage::{
    ARTIFACT_ITEMS_LIMIT, ARTIFACT_LABELS_LIMIT, ARTIFACT_TEXT_LIMIT, Error, FixtureArtifact,
    MANIFEST_BYTES_LIMIT, ManifestLengths, Registered, Registrant,
};
use std::time::Duration;
use task_writer_support::{put, running};

const CALLER: Registrant = Registrant::Caller;
const ENGINE: Registrant = Registrant::Engine;

fn limit(e: &Error) -> bool {
    matches!(e, Error::LimitExceeded)
}

fn nth(n: u64) -> ArtifactItem {
    ArtifactItem {
        registered: observation(n),
        ..item(&format!("a{n}"), &format!("/p/{n}"))
    }
}

fn stored_lengths(w: &World, id: &str) -> i64 {
    one(
        &w.db(),
        "SELECT sum(encoded_len) FROM artifact_items WHERE task=?1",
        [id],
    )
}

#[test]
fn the_exported_bounds_are_the_reference_values() {
    assert_eq!(
        (ARTIFACT_ITEMS_LIMIT, MANIFEST_BYTES_LIMIT),
        (100, MANIFEST_MAX)
    );
    assert_eq!((ARTIFACT_TEXT_LIMIT, ARTIFACT_LABELS_LIMIT), (4096, 1024));
}

#[test]
fn a_hundred_items_land_and_the_hundred_and_first_is_refused() {
    let w = world();
    put(&w, &running(A));
    for n in 0..100 {
        added(
            &w,
            A,
            &nth(n),
            usual(),
            if n % 2 == 0 { CALLER } else { ENGINE },
        );
    }
    assert_eq!(items(&w, A).len(), 100);
    assert_eq!(stored_lengths(&w, A), 100 * ITEM_A as i64);
    for who in [CALLER, ENGINE] {
        refused(&w, A, || register(&w, A, &nth(100), usual(), who), limit);
    }
    // The duplicate lookup precedes the cap.
    let Registered::Existing(found) = register(&w, A, &nth(42), usual(), CALLER).unwrap() else {
        panic!("expected a duplicate");
    };
    assert_eq!(found.artifact_id, "a42");
}

/// Registers items of `sizes` to a fresh running task, then the last length exactly at the
/// bound, then checks one byte more is refused on an equal second task.
fn edge(sizes: &[u64]) {
    let w = world();
    put(&w, &running(A));
    put(&w, &running(B));
    for id in [A, B] {
        for (n, size) in sizes.iter().enumerate() {
            added(&w, id, &nth(n as u64), lengths(ENVELOPE_A, *size), ENGINE);
        }
    }
    let separators = 2 * sizes.len() as u64;
    let last = MANIFEST_MAX - ENVELOPE_A - sizes.iter().sum::<u64>() - separators;
    let at_edge = lengths(ENVELOPE_A, last);
    let over = lengths(ENVELOPE_A, last + 1);
    refused(&w, B, || register(&w, B, &nth(99), over, ENGINE), limit);
    added(&w, A, &nth(99), at_edge, ENGINE);
    let total = ENVELOPE_A as i64 + stored_lengths(&w, A) + separators as i64;
    assert_eq!(total, MANIFEST_MAX as i64, "{sizes:?}");
    // The manifest is now full to the byte: even a one-byte item is refused.
    refused(
        &w,
        A,
        || register(&w, A, &nth(98), lengths(ENVELOPE_A, 1), ENGINE),
        limit,
    );
    added(&w, B, &nth(99), at_edge, ENGINE);
}

#[test]
fn a_manifest_of_exactly_1_048_576_bytes_lands_and_one_byte_more_is_refused() {
    edge(&[]);
    edge(&[ITEM_A]);
    edge(&[ITEM_A, ITEM_A, 362]);
    edge(&[500_000, 7]);
}

#[test]
fn an_envelope_and_item_alone_over_the_bound_are_refused_before_the_task_lookup() {
    let w = world();
    put(&w, &running(A));
    let cases = [
        lengths(MANIFEST_MAX, 1),
        lengths(1, MANIFEST_MAX),
        lengths(MANIFEST_MAX + 1, 0),
        lengths(0, i64::MAX as u64 + 1),
        lengths(u64::MAX, u64::MAX),
    ];
    for at in cases {
        for id in [A, C] {
            refused(
                &w,
                id,
                || register(&w, id, &item("a", "/p"), at, CALLER),
                limit,
            );
        }
    }
    // The bound is inclusive, before and inside the transaction.
    added(&w, A, &item("a", "/p"), lengths(0, MANIFEST_MAX), CALLER);
}

/// The item over its bound is refused on a valid and a missing task (so before the lookup);
/// the one at its bound lands.
fn value_edge(w: &World, ok: ArtifactItem, over: ArtifactItem) {
    for id in [A, C] {
        refused(w, id, || register(w, id, &over, usual(), ENGINE), limit);
    }
    added(w, A, &ok, usual(), ENGINE);
}

/// A distinct inode for every edge item, so none deduplicates against another.
fn next_inode() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1000);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn unique(mut built: ArtifactItem) -> ArtifactItem {
    built.registered = observation(next_inode());
    built
}

fn with(change: impl Fn(&mut ArtifactItem)) -> ArtifactItem {
    let mut built = unique(item("w", "/p"));
    change(&mut built);
    built
}

#[test]
fn path_note_and_label_bytes_are_bounded_before_the_task_lookup() {
    let w = world();
    put(&w, &running(A));
    let path = |n: usize| with(|i| i.path = format!("/{}", "p".repeat(n - 1)));
    value_edge(&w, path(4096), path(4097));
    let note = |extra: &str| with(|i| i.note = format!("{}{extra}", "\u{e9}".repeat(2048)));
    value_edge(&w, note(""), note("x"));
    // kind and registered_at are 6 + 25 of the 1024 bytes; the method is 24 by default.
    let id = |n: usize| with(|i| i.artifact_id = "i".repeat(n));
    value_edge(&w, id(969), id(970));
    let method = |n: usize| {
        with(|i| {
            i.artifact_id = "i".into();
            i.method = "m".repeat(n);
        })
    };
    value_edge(&w, method(992), method(993));
}

#[test]
fn plan_evidence_bytes_are_bounded_before_the_task_lookup() {
    let w = world();
    put(&w, &running(A));
    let tool = |n: usize| {
        let mut built = unique(plan_item("t", None));
        built.plan.as_mut().unwrap().tool_use_id = "t".repeat(n);
        built
    };
    value_edge(&w, tool(256), tool(257));
    value_edge(&w, tool(1), tool(0));
    let directory = |n: usize| unique(plan_item("d", Some((&"d".repeat(n), 1, 2))));
    value_edge(&w, directory(4096), directory(4097));
    value_edge(&w, directory(1), directory(0));
}

#[test]
fn a_size_beyond_a_sqlite_integer_is_refused_before_the_task_lookup() {
    let w = world();
    put(&w, &running(A));
    let size = |n: u64| {
        let mut built = unique(item("s", "/p"));
        built.registered.size = n;
        built
    };
    value_edge(&w, size(i64::MAX as u64), size(i64::MAX as u64 + 1));
    let stored: String = one(
        &w.db(),
        "SELECT quote(size) FROM artifact_items WHERE task=?1",
        [A],
    );
    assert_eq!(stored, "9223372036854775807");
}

fn fixture(id: &str, n: usize, encoded_len: u64) -> FixtureArtifact {
    FixtureArtifact {
        encoded_len,
        ..artifact_item(id, &format!("/f/{}", n % 2))
    }
}

#[test]
fn the_fixture_replace_bypasses_duplicates_the_cap_and_the_size_bound() {
    let w = world();
    put(&w, &running(A));
    put(&w, &running(B));
    // 101 items, pairwise equal, together far over the serialized bound.
    let many: Vec<FixtureArtifact> = (0..101)
        .map(|n| fixture(&format!("f{n}"), n, 20_000))
        .collect();
    w.store
        .replace_artifacts_fixture(&tid(A), true, &many, later(5))
        .unwrap();
    assert_eq!(items(&w, A).len(), 101);
    assert_eq!(stored_lengths(&w, A), 101 * 20_000);
    refused(
        &w,
        A,
        || register(&w, A, &item("n", "/p/new"), lengths(ENVELOPE_A, 1), CALLER),
        limit,
    );
    let duplicate = item("d", "/f/1");
    let Registered::Existing(found) = register(&w, A, &duplicate, usual(), CALLER).unwrap() else {
        panic!("expected a duplicate");
    };
    assert_eq!(found.artifact_id, "f1");
    // Two items whose stored lengths already exceed the bound refuse any further item.
    let big = [fixture("g0", 0, 600_000), fixture("g1", 1, 600_000)];
    w.store
        .replace_artifacts_fixture(&tid(B), true, &big, later(5))
        .unwrap();
    refused(
        &w,
        B,
        || register(&w, B, &item("n", "/p/new"), lengths(ENVELOPE_A, 1), ENGINE),
        limit,
    );
    // A length beyond a SQLite integer is refused and nothing is replaced.
    let before = items(&w, B);
    let error = w
        .store
        .replace_artifacts_fixture(
            &tid(B),
            true,
            &[fixture("h", 0, i64::MAX as u64 + 1)],
            later(5),
        )
        .unwrap_err();
    assert!(limit(&error), "{error:?}");
    assert_eq!(items(&w, B), before);
}

#[test]
fn concurrent_registrations_at_the_byte_edge_admit_exactly_one() {
    let w = world();
    put(&w, &running(A));
    added(&w, A, &nth(0), usual(), ENGINE);
    let room = MANIFEST_MAX - ENVELOPE_A - ITEM_A - 2;
    let half: ManifestLengths = lengths(ENVELOPE_A, room / 2 + 1);
    let outcomes: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = (1..=4)
            .map(|n| {
                let w = &w;
                scope.spawn(move || {
                    // One store admits one write at a time; a `Busy` admission is retried, so
                    // every registration reaches the in-transaction bound check.
                    loop {
                        match register(w, A, &nth(n), half, ENGINE) {
                            Err(Error::Busy) => std::thread::sleep(Duration::from_millis(1)),
                            outcome => break outcome,
                        }
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let landed = outcomes
        .iter()
        .filter(|o| matches!(o, Ok(Registered::Added)))
        .count();
    let over = outcomes
        .iter()
        .filter(|o| matches!(o, Err(Error::LimitExceeded)))
        .count();
    assert_eq!((landed, over), (1, 3), "{outcomes:?}");
    assert_eq!(items(&w, A).len(), 2);
}
