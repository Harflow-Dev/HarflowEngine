#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Four writer processes commit concurrently under the Owner policy, so their WAL
//! maintenance interleaves with each other's commits. Busy is retried; an ambiguous
//! commit is never acceptable. Uses the test lease (no production lease is needed).
mod common;
mod wal_owner_support;
use common::{Fixture, default_limits};
use hf_storage::{Error, Mutation, PASSIVE_AFTER, Store, WriterOptions};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use wal_owner_support::{ONE_COMMIT, initialized, wal_len};

const WRITERS: usize = 4;
const COMMITS: usize = 20;
const VALUE: usize = 300_000;

fn retry<T>(until: Instant, mut action: impl FnMut() -> hf_storage::Result<T>) -> Result<T, Error> {
    loop {
        match action() {
            Err(Error::Busy | Error::DeadlineExceeded) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(2));
            }
            other => return other,
        }
    }
}

fn child_run(root: &str, writer: &str, until: Instant) -> Result<usize, Error> {
    let soon = || Instant::now() + Duration::from_secs(2);
    let store = retry(until, || {
        Store::open_with(
            root,
            default_limits(),
            soon(),
            common::lease::lease_port(),
            WriterOptions::owner(),
        )
    })?;
    for commit in 0..COMMITS {
        let mutation = Mutation::Put {
            key: format!("{writer}-{commit}"),
            value: vec![u8::try_from(commit).unwrap(); VALUE],
        };
        retry(until, || {
            store.apply_batch(std::slice::from_ref(&mutation), soon())
        })?;
    }
    Ok(COMMITS)
}

#[test]
fn wal_owner_writer_child() {
    let Ok(root) = std::env::var("HF_WAL_OWNER_CHILD_ROOT") else {
        return;
    };
    let writer = std::env::var("HF_WAL_OWNER_CHILD_ID").unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    match child_run(&root, &writer, until) {
        Ok(count) => println!("RESULT ok {count}"),
        Err(error) => println!("RESULT error {error:?}"),
    }
}

#[test]
fn four_owner_writer_processes_never_report_an_ambiguous_commit() {
    let scratch = Fixture::new();
    let root = initialized(&scratch);
    let children: Vec<_> = (0..WRITERS)
        .map(|writer| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "wal_owner_writer_child", "--nocapture"])
                .env("HF_WAL_OWNER_CHILD_ROOT", &root)
                .env("HF_WAL_OWNER_CHILD_ID", format!("w{writer}"))
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(output.status.success(), "{stdout}");
        let result = stdout
            .lines()
            .find_map(|line| line.strip_prefix("RESULT "))
            .unwrap_or_else(|| panic!("no result: {stdout}"));
        assert!(!result.contains("CommitOutcomeUnknown"), "{result}");
        assert_eq!(result, format!("ok {COMMITS}"));
    }
    let length = wal_len(&root);
    assert!(length <= PASSIVE_AFTER + 2 * ONE_COMMIT, "{length}");
    let store = Store::open(
        &root,
        default_limits(),
        common::deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    for writer in 0..WRITERS {
        for commit in 0..COMMITS {
            let key = format!("w{writer}-{commit}");
            let records = store
                .read_batch(std::slice::from_ref(&key), common::deadline())
                .unwrap();
            assert_eq!(records.len(), 1, "{key} is missing");
            assert_eq!(records[0].value, vec![u8::try_from(commit).unwrap(); VALUE]);
        }
    }
}
