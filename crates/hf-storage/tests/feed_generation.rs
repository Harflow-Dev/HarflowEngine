#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod feed_support;
mod read_session_support;
#[allow(unused_imports)]
use common::{deadline, default_limits};
#[allow(unused_imports)]
use feed_support::*;
#[allow(unused_imports)]
use hf_application::ctl::task_reads::ScanRow;
#[allow(unused_imports)]
use hf_domain::{
    ctl::statistics::{GenerationEntry, StatFacts},
    event_read::CaptureState,
    task_read::{Coverage, TaskId},
};
#[allow(unused_imports)]
use hf_storage::{Error, FixtureLine, Limits, ReadSession, Store};
#[allow(unused_imports)]
use read_session_support::edit_main;
#[allow(unused_imports)]
use sha2::{Digest, Sha256};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

fn entry(name: &str, only_identity: bool) -> GenerationEntry {
    let mut found = hf_domain::ctl::statistics::GenerationEntry {
        facts: StatFacts {
            id: id(name),
            status: "completed".into(),
            started_at: "x".into(),
            started_at_us: 1_768_039_200_000_000,
            finished_at: Some("y".into()),
            finished_at_us: Some(1_768_039_500_000_000),
            exit_code: Some(0),
            title: Some("t".into()),
            backend: "claude".into(),
            model: None,
            repo_path: "/repo".into(),
            continuation_of: None,
            chain_root: hf_domain::ctl::task_rows::ChainRoot::Absent,
        },
        identity_only: only_identity,
    };
    found.identity_only = only_identity;
    found
}

fn generation_of(root: &Root) -> String {
    std::fs::read_to_string(root.path.join("store.identity"))
        .unwrap()
        .lines()
        .nth(2)
        .unwrap()
        .to_string()
}

#[test]
fn the_generation_is_the_sha256_of_the_documented_json() {
    let root = root(names(1));
    let identity = generation_of(&root);
    let digest = read(&root, host(), |s| {
        s.statistics_generation(&[entry("a", false), entry("b", true)])
    })
    .unwrap();
    let text = format!(
        "[\"rs-statistics-1\",\"{identity}\",[[\"task:a\",\"facts\",\"completed\",1768039200000000,1768039500000000,0,\"t\",\"claude\",null,\"/repo\",null,null],[\"task:b\",\"nonterminal_ancestry\",null,null]]]"
    );
    let expected: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(digest, expected);
}

#[test]
fn the_generation_follows_every_fact_but_only_the_identity_of_a_running_ancestor() {
    let root = root(names(1));
    let digest = |entries: Vec<GenerationEntry>| {
        read(&root, host(), |s| s.statistics_generation(&entries)).unwrap()
    };
    let base = digest(vec![entry("a", false)]);
    assert_eq!(base, digest(vec![entry("a", false)]));
    type Change = Box<dyn Fn(&mut GenerationEntry)>;
    let mut changes: Vec<Change> = vec![
        Box::new(|e| e.facts.status = "failed".into()),
        Box::new(|e| e.facts.started_at_us += 1),
        Box::new(|e| e.facts.finished_at_us = None),
        Box::new(|e| e.facts.exit_code = None),
        Box::new(|e| e.facts.title = None),
        Box::new(|e| e.facts.backend = "codex".into()),
        Box::new(|e| e.facts.model = Some("m".into())),
        Box::new(|e| e.facts.repo_path = "/other".into()),
        Box::new(|e| e.facts.continuation_of = Some(id("p"))),
        Box::new(|e| e.facts.chain_root = hf_domain::ctl::task_rows::ChainRoot::Valid(id("p"))),
        Box::new(|e| e.facts.id = id("b")),
    ];
    for change in changes.drain(..) {
        let mut changed = entry("a", false);
        change(&mut changed);
        assert_ne!(digest(vec![changed]), base);
    }
    let mut text_only = entry("a", false);
    text_only.facts.started_at = "another spelling".into();
    text_only.facts.finished_at = None;
    assert_eq!(
        digest(vec![text_only]),
        base,
        "the instants are the microsecond columns"
    );
    let identity = digest(vec![entry("a", true)]);
    let mut moved = entry("a", true);
    (
        moved.facts.status,
        moved.facts.title,
        moved.facts.started_at_us,
    ) = ("running".into(), None, 1);
    assert_eq!(digest(vec![moved]), identity);
    let mut linked = entry("a", true);
    linked.facts.continuation_of = Some(id("p"));
    assert_ne!(digest(vec![linked]), identity);
    assert_ne!(
        digest(vec![entry("a", false), entry("b", false)]),
        digest(vec![entry("b", false), entry("a", false)])
    );
}

#[test]
fn another_store_has_another_generation_for_the_same_facts() {
    let (first, second) = (root(names(1)), root(names(1)));
    let entries = [entry("a", false)];
    let one = read(&first, host(), |s| s.statistics_generation(&entries)).unwrap();
    let two = read(&second, host(), |s| s.statistics_generation(&entries)).unwrap();
    assert_ne!(generation_of(&first), generation_of(&second));
    assert_ne!(one, two);
}
