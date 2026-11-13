#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The verification cadence: when `store.verify` makes a check due, and what a
//! passing check writes there.
mod common;
mod read_session_support;
mod verify_support;
use hf_storage::{Error, VERIFY_PERIOD, Verification};
use read_session_support::{later, seeded};
use std::fs;
use verify_support::{
    FENCE, PERIOD, VERIFY, at, clear, due_bytes, exists, generation, open_store, verification_files,
};

const NOW: u64 = 1_800_000_000;

fn put_due(root: &std::path::Path, due: &str) {
    fs::write(root.join(VERIFY), due_bytes(&generation(root), due)).unwrap();
}

/// A labelled way to occupy a file name.
type Plant<'a> = (&'static str, Box<dyn Fn() + 'a>);

#[test]
fn the_period_is_one_day() {
    assert_eq!(VERIFY_PERIOD.as_secs(), PERIOD);
}

#[test]
fn an_absent_record_is_due_and_a_pass_writes_the_next_due_time() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    assert!(store.verification_due(at(NOW)).unwrap());
    assert!(!exists(root, VERIFY), "a due query writes nothing");
    assert_eq!(
        store.verify_if_due(at(NOW), later(30)).unwrap(),
        Some(Verification::Passed)
    );
    let expected = due_bytes(&generation(root), &(NOW + PERIOD).to_string());
    assert_eq!(fs::read(root.join(VERIFY)).unwrap(), expected);
    assert_eq!(verification_files(root), vec![VERIFY.to_string()]);
    // Not due again until the period has passed.
    assert_eq!(store.verify_if_due(at(NOW + 1), later(30)).unwrap(), None);
    assert_eq!(fs::read(root.join(VERIFY)).unwrap(), expected);
}

#[test]
fn the_due_time_is_inclusive_and_a_clock_moved_back_is_due() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let due = NOW + PERIOD;
    put_due(root, &due.to_string());
    let cases = [
        ("just before", due - 1, false),
        ("exactly due", due, true),
        ("after", due + 1, true),
        ("a full period ahead", NOW, false),
        ("more than a period ahead", NOW - 1, true),
        ("long before", 0, true),
    ];
    for (label, now, want) in cases {
        assert_eq!(store.verification_due(at(now)).unwrap(), want, "{label}");
    }
    assert_eq!(
        store.verify_if_due(at(due - 1), later(30)).unwrap(),
        None,
        "not due"
    );
    assert_eq!(
        fs::read(root.join(VERIFY)).unwrap(),
        due_bytes(&generation(root), &due.to_string())
    );
}

#[test]
fn a_pass_rewrites_an_expired_or_backward_due_time() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    for (label, due) in [
        ("expired", NOW - 10),
        ("clock moved back", NOW + 3 * PERIOD),
    ] {
        put_due(root, &due.to_string());
        assert_eq!(
            store.verify_if_due(at(NOW), later(30)).unwrap(),
            Some(Verification::Passed),
            "{label}"
        );
        assert_eq!(
            fs::read(root.join(VERIFY)).unwrap(),
            due_bytes(&generation(root), &(NOW + PERIOD).to_string()),
            "{label}"
        );
    }
}

#[test]
fn a_forced_check_passes_and_rewrites_a_record_that_was_not_due() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let wall = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };
    let ahead = wall() + PERIOD / 2;
    put_due(root, &ahead.to_string());
    assert!(
        !store
            .verification_due(std::time::SystemTime::now())
            .unwrap()
    );
    let before = wall();
    assert_eq!(
        store.verify_in_place(later(30)).unwrap(),
        Verification::Passed
    );
    let after = wall();
    let text = fs::read_to_string(root.join(VERIFY)).unwrap();
    let due: u64 = text.lines().nth(2).unwrap().parse().unwrap();
    assert!(
        (before + PERIOD..=after + PERIOD).contains(&due),
        "rewritten from the real clock: {due}"
    );
    assert!(!exists(root, FENCE));
}

#[test]
fn malformed_and_foreign_records_are_due() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let generation = generation(root);
    let far = (NOW + PERIOD / 2).to_string();
    let valid = String::from_utf8(due_bytes(&generation, &far)).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("foreign generation", due_bytes(&"f".repeat(32), &far)),
        ("empty", Vec::new()),
        ("wrong header", valid.replace("-1\n", "-2\n").into_bytes()),
        ("no trailing newline", valid.trim_end().as_bytes().to_vec()),
        ("extra line", format!("{valid}extra\n").into_bytes()),
        ("leading zero", due_bytes(&generation, &format!("0{far}"))),
        ("sign", due_bytes(&generation, &format!("+{far}"))),
        ("negative", due_bytes(&generation, "-1")),
        ("space", due_bytes(&generation, &format!(" {far}"))),
        ("empty due", due_bytes(&generation, "")),
        (
            "overflow",
            due_bytes(&generation, "99999999999999999999999"),
        ),
        ("crlf", valid.replace('\n', "\r\n").into_bytes()),
        ("128 bytes", {
            let mut bytes = valid.clone().into_bytes();
            bytes.resize(128, b'x');
            bytes
        }),
    ];
    // Each case would not be due if it were read as the valid record it resembles.
    put_due(root, &far);
    assert!(!store.verification_due(at(NOW)).unwrap(), "control");
    for (label, bytes) in cases {
        fs::write(root.join(VERIFY), &bytes).unwrap();
        assert!(store.verification_due(at(NOW)).unwrap(), "{label}");
    }
    // A malformed record is replaced by the next pass.
    assert_eq!(
        store.verify_if_due(at(NOW), later(30)).unwrap(),
        Some(Verification::Passed)
    );
    assert!(!store.verification_due(at(NOW)).unwrap());
}

#[test]
fn an_oversized_regular_record_is_due_and_replaced_by_a_pass() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let mut oversized = due_bytes(&generation(root), &(NOW + 10).to_string());
    oversized.resize(129, b'\n');
    fs::write(root.join(VERIFY), &oversized).unwrap();
    assert!(store.verification_due(at(NOW)).unwrap());
    assert_eq!(
        store.verify_if_due(at(NOW), later(30)).unwrap(),
        Some(Verification::Passed)
    );
    assert!(fs::read(root.join(VERIFY)).unwrap().len() <= 128);
    assert!(!store.verification_due(at(NOW)).unwrap());
    assert!(!exists(root, FENCE));
}

#[test]
fn a_non_regular_record_is_incompatible() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let elsewhere = seeded.scratch.path().join("elsewhere-verify");
    fs::write(
        &elsewhere,
        due_bytes(&generation(root), &(NOW + 10).to_string()),
    )
    .unwrap();
    let plants: [Plant<'_>; 2] = [
        (
            "directory",
            Box::new(|| fs::create_dir(root.join(VERIFY)).unwrap()),
        ),
        (
            "symlink to a valid record",
            Box::new(|| std::os::unix::fs::symlink(&elsewhere, root.join(VERIFY)).unwrap()),
        ),
    ];
    for (label, plant) in plants {
        plant();
        assert!(
            matches!(store.verification_due(at(NOW)), Err(Error::Incompatible)),
            "{label}"
        );
        assert!(
            matches!(
                store.verify_if_due(at(NOW), later(30)),
                Err(Error::Incompatible)
            ),
            "{label}"
        );
        clear(root, VERIFY);
    }
    assert_eq!(
        fs::read(&elsewhere).unwrap(),
        due_bytes(&generation(root), &(NOW + 10).to_string()),
        "the link target is untouched"
    );
    assert!(!exists(root, FENCE));
}

#[test]
fn a_time_before_the_epoch_counts_as_zero() {
    let seeded = seeded(3);
    let root = &seeded.root;
    let store = open_store(root).unwrap();
    let before = std::time::UNIX_EPOCH - std::time::Duration::from_secs(5);
    put_due(root, "0");
    assert!(
        store.verification_due(before).unwrap(),
        "zero is due at zero"
    );
    put_due(root, &PERIOD.to_string());
    assert!(
        !store.verification_due(before).unwrap(),
        "one period ahead of zero"
    );
    assert_eq!(
        store.verify_if_due(at(PERIOD), later(30)).unwrap(),
        Some(Verification::Passed)
    );
}
