#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The artifact page at its byte bound: 63,000 bytes of the candidate the reference measures,
//! with and without a clock.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_domain::ctl::{
    caller::PidObservation,
    history::{ArtifactItem, ManifestHead, Observation},
};
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::{Value, json};

const BOUND: usize = 63_000;
const REVISION: &str = "0123456789abcdef0123456789abcdef";
/// A stamp as wide as any the reference writes: the microseconds are present.
const FULL_STAMP: &str = "1970-01-01T00:00:01.000001+00:00";

fn item(n: usize, note: usize) -> ArtifactItem {
    ArtifactItem {
        artifact_id: format!("x{n:02}"),
        path: format!("/repo/out/f{n:02}.txt"),
        kind: "report".into(),
        note: "n".repeat(note),
        registered_at: "2026-01-01T00:00:00+00:00".into(),
        method: "verified_process_lineage".into(),
        registered: Observation {
            device: 1,
            inode: n as u64,
            size: 3,
            mtime_ns: 4,
        },
    }
}

fn scripted(clock: bool, count: usize, last: usize, extra: usize) -> Fake {
    let mut fake = fake("completed", Some(0), PidObservation::Absent);
    fake.clock = clock;
    fake.history.manifest = Some(ManifestHead {
        header_valid: true,
        revision: REVISION.into(),
        count,
    });
    fake.history.items = (0..count)
        .map(|n| item(n, if n == last { 1_000 + extra } else { 1_000 }))
        .collect();
    fake
}

fn page(fake: &Fake) -> Value {
    let out = dispatch(
        &args(&["task-artifacts", ID, "--json", "--limit", "50"]),
        fake,
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    serde_json::from_str::<Value>(&out.stdout).unwrap()["result"].clone()
}

fn size(value: &Value) -> usize {
    match value {
        Value::Array(items) => {
            2 + items.iter().map(size).sum::<usize>() + 2 * items.len().saturating_sub(1)
        }
        Value::Object(map) => {
            2 + map
                .iter()
                .map(|(k, v)| size(&json!(k)) + 2 + size(v))
                .sum::<usize>()
                + 2 * map.len().saturating_sub(1)
        }
        Value::String(text) => 2 + text.len(),
        other => other.to_string().len(),
    }
}

/// The candidate as the reference measures it, the stamps replaced by `stamp` where the page has none.
fn candidate(result: &Value, stamp: Option<&str>) -> usize {
    let mut items = result["items"].clone();
    if let Some(stamp) = stamp {
        for entry in items.as_array_mut().unwrap() {
            entry["availability"]["observed_at"] = json!(stamp);
        }
    }
    size(
        &json!({"task_id": result["task_id"], "items": items, "next_cursor": null, "has_more": false,
                 "state": "available", "count": 0}),
    )
}

fn at_the_bound(clock: bool) {
    let stamp = (!clock).then_some(FULL_STAMP);
    let first = page(&scripted(clock, 50, 0, 0));
    let count = first["items"].as_array().unwrap().len();
    assert!((30..50).contains(&count), "{count}");
    let slack = BOUND - candidate(&first, stamp);
    let exact = page(&scripted(clock, 50, count - 1, slack));
    assert_eq!(exact["items"].as_array().unwrap().len(), count);
    assert_eq!(candidate(&exact, stamp), BOUND);
    let over = page(&scripted(clock, 50, count - 1, slack + 1));
    assert_eq!(over["items"].as_array().unwrap().len(), count - 1);
    assert!(candidate(&over, stamp) <= BOUND);
    assert_eq!(over["has_more"], true);
}

#[test]
fn the_bound_is_exact_with_a_clock() {
    at_the_bound(true);
}

#[test]
fn the_bound_is_exact_without_a_clock_where_the_stamp_is_counted_full_width() {
    at_the_bound(false);
}

#[test]
fn without_a_clock_the_page_is_shorter_than_the_stamps_it_prints_would_allow() {
    let with = page(&scripted(true, 50, 0, 0));
    let without = page(&scripted(false, 50, 0, 0));
    assert!(without["items"].as_array().unwrap().len() <= with["items"].as_array().unwrap().len());
    assert_eq!(
        without["items"][0]["availability"]["observed_at"],
        Value::Null
    );
}
