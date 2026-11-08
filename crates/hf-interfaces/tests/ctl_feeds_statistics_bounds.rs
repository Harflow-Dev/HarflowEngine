#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The statistics page at its byte bounds: 258,048 bytes for the candidate of a page, 4,096 for
//! the cursor and 262,144 for the document, each measured the way the reference engine measures
//! it (`json.dumps` with its separators and `ensure_ascii`) on the document the engine prints.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_application::ctl::task_reads::ScanRow;
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::{Value, json};

const CANDIDATE: usize = 258_048;
const DOCUMENT: usize = 262_144;
const FIRST: i64 = 1_768_039_200_000_000;

/// `json.dumps(value)` with default or compact separators; every string here is ASCII.
fn size(value: &Value, compact: bool) -> usize {
    let (item, key) = if compact { (1, 1) } else { (2, 2) };
    match value {
        Value::Array(items) => {
            2 + items.iter().map(|v| size(v, compact)).sum::<usize>()
                + item * items.len().saturating_sub(1)
        }
        Value::Object(map) => {
            2 + map
                .iter()
                .map(|(k, v)| 2 + k.len() + key + size(v, compact))
                .sum::<usize>()
                + item * map.len().saturating_sub(1)
        }
        Value::String(text) => 2 + text.len(),
        Value::Number(number) => number.to_string().len(),
        other => other.to_string().len(),
    }
}

fn scripted(pads: &[usize]) -> Fake {
    let mut fake = fake(
        "completed",
        Some(0),
        hf_domain::ctl::caller::PidObservation::Absent,
    );
    fake.feed.rows = pads
        .iter()
        .enumerate()
        .map(|(n, pad)| {
            let mut found = facts(
                &format!("t{n:03}"),
                FIRST + n as i64 * 1_000_000,
                FIRST + n as i64 * 1_000_000 + 300_000_000,
            );
            found.repo_path = "r".repeat(*pad);
            ScanRow::Facts(Box::new(found))
        })
        .collect();
    fake
}

fn page(fake: &Fake) -> Value {
    let args = args(&[
        "statistics-page",
        "--since",
        "2026-01-01T00:00:00Z",
        "--before",
        "2027-01-01T00:00:00Z",
        "--limit",
        "100",
        "--json",
    ]);
    let out = dispatch(&args, fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let whole: Value = serde_json::from_str(&out.stdout).unwrap();
    // The printed bytes are the size the bound is measured in: default separators, plus the newline.
    assert_eq!(out.stdout.len(), size(&whole, false) + 1);
    whole["result"].clone()
}

#[test]
fn the_candidate_bound_is_258048_exact() {
    let probe = page(&scripted(&[128_000, 1000]));
    assert_eq!(probe["items"].as_array().unwrap().len(), 2);
    let slack = CANDIDATE - size(&probe, true);
    let exact = page(&scripted(&[128_000, 1000 + slack]));
    assert_eq!(size(&exact, true), CANDIDATE);
    assert_eq!(exact["items"].as_array().unwrap().len(), 2);
    assert_eq!(exact["has_more"], false);
    let over = page(&scripted(&[128_000, 1000 + slack + 1]));
    assert_eq!(over["state"], "ready");
    assert_eq!(over["items"].as_array().unwrap().len(), 1);
    assert_eq!(over["has_more"], true);
    assert!(over["next_cursor"].is_string());
    assert!(size(&json!({"items": over["items"]}), true) < CANDIDATE);
}

#[test]
fn the_document_bound_is_262144_exact_and_a_byte_more_is_page_limit() {
    let mut pads = vec![0usize; 100];
    let probe = page(&scripted(&pads));
    assert_eq!(probe["items"].as_array().unwrap().len(), 100);
    let plain = size(&probe, false);
    assert!(plain < DOCUMENT, "{plain}");
    // Spread the padding: one source over 128 KiB is not projected.
    let total = DOCUMENT - plain;
    (pads[97], pads[98], pads[99]) = (total / 3, total / 3, total - 2 * (total / 3));
    let exact = page(&scripted(&pads));
    assert_eq!(exact["state"], "ready");
    assert_eq!(size(&exact, false), DOCUMENT);
    assert!(
        size(&exact, true) <= CANDIDATE,
        "the candidate bound would have stopped the page first"
    );
    pads[99] += 1;
    let over = page(&scripted(&pads));
    assert_eq!(over["state"], "unavailable");
    assert_eq!(
        over["diagnostics"],
        json!([{"code": "page_limit", "message": "Statistics page byte budget exceeded"}])
    );
    assert_eq!(over["items"], json!([]));
}

#[test]
fn a_cursor_of_more_than_4096_characters_is_page_limit_and_one_of_4096_is_not() {
    let mut fake = scripted(&[0, 0]);
    let args = args(&[
        "statistics-page",
        "--since",
        "2026-01-01T00:00:00Z",
        "--before",
        "2027-01-01T00:00:00Z",
        "--limit",
        "1",
        "--json",
    ]);
    let cursor_len = |root_len: usize, fake: &mut Fake| {
        fake.root = format!("/{}", "r".repeat(root_len - 1));
        let out = dispatch(&args, fake);
        assert_eq!(out.code, 0, "{}", out.stderr);
        let result = serde_json::from_str::<Value>(&out.stdout).unwrap()["result"].clone();
        (
            result["state"].as_str().unwrap().to_string(),
            result["next_cursor"].as_str().map(str::len),
            result["diagnostics"].clone(),
        )
    };
    let (state, small, _) = cursor_len(100, &mut fake);
    assert_eq!((state.as_str(), small.is_some()), ("ready", true));
    // The text grows by four characters for each three bytes of root: find the last root that fits.
    let mut fit = 2500;
    while cursor_len(fit + 3, &mut fake).1.is_some_and(|n| n <= 4096) {
        fit += 3;
    }
    let (state, length, _) = cursor_len(fit, &mut fake);
    assert_eq!(
        (state.as_str(), length.is_some_and(|n| n <= 4096)),
        ("ready", true)
    );
    let mut over = None;
    for extra in 1..=6 {
        let (state, length, diagnostics) = cursor_len(fit + extra, &mut fake);
        if state == "unavailable" {
            over = Some(diagnostics);
            break;
        }
        assert!(length.unwrap() <= 4096);
    }
    assert_eq!(
        over.unwrap(),
        json!([{"code": "page_limit", "message": "Statistics cursor byte budget exceeded"}])
    );
}

/// The days of a retention of 10**30: the digits are printed out of band, so a parsed document
/// counts them as the float 1e30 and the difference is added back.
const DAYS: &str = "1000000000000000000000000000000";

fn with_days(pads: &[usize]) -> (Value, usize) {
    let mut fake = scripted(pads);
    fake.retention = Some(DAYS.to_string());
    let args = args(&[
        "statistics-page",
        "--since",
        "2026-01-01T00:00:00Z",
        "--before",
        "2027-01-01T00:00:00Z",
        "--limit",
        "100",
        "--json",
    ]);
    let out = dispatch(&args, &fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.contains(&format!("\"days\": {DAYS}")),
        "{}",
        out.stdout
    );
    let whole: Value = serde_json::from_str(&out.stdout).unwrap();
    let parsed = whole["result"]["retention"]["days"].to_string().len();
    let fix = DAYS.len() - parsed;
    // What is printed is what is sized: the whole document, newline included.
    assert_eq!(out.stdout.len(), size(&whole, false) + 1 + fix);
    // The bound is on the result alone.
    (whole["result"].clone(), size(&whole["result"], false) + fix)
}

#[test]
fn a_retention_beyond_64_bits_is_sized_as_it_is_printed_and_the_bound_stays_exact() {
    let mut pads = vec![0usize; 100];
    let (probe, sized) = with_days(&pads);
    assert_eq!(probe["state"], "ready");
    let total = DOCUMENT - sized;
    (pads[97], pads[98], pads[99]) = (total / 3, total / 3, total - 2 * (total / 3));
    let (exact, sized) = with_days(&pads);
    assert_eq!((exact["state"].as_str(), sized), (Some("ready"), DOCUMENT));
    pads[99] += 1;
    let (over, _) = with_days(&pads);
    assert_eq!(over["state"], "unavailable");
}
