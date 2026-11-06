//! Deterministic CI/large dataset generator over the existing fixture APIs.
use hf_application::{
    event_read::{EventRecord, RecordClass},
    task_read::TaskRow,
};
use hf_domain::{
    event_read::{CaptureState, LogFacts},
    task_read::{Coverage, StoredObservation, TaskId},
};
use hf_storage::{EventFixture, Limits, Store, TaskFixture};
use std::{path::Path, sync::Arc, time::Duration, time::Instant};

pub const SEED: u64 = 0x4846_5332;
const KINDS: [&str; 10] = [
    "task_started",
    "assistant_text",
    "assistant_delta",
    "tool_call",
    "tool_result",
    "user_message",
    "usage",
    "notice",
    "task_finished",
    "undelivered",
];

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.below(hi - lo + 1)
    }
    pub fn bytes(&mut self, len: u64) -> Vec<u8> {
        (0..len).map(|_| b'a' + (self.below(26) as u8)).collect()
    }
}

pub fn task_id(index: u64) -> TaskId {
    TaskId::new(format!("t-{index:08x}")).unwrap()
}

pub fn limits() -> Limits {
    Limits {
        max_rows: 1_000_000,
        max_bytes: 256 * 1024 * 1024,
    }
}

fn flag(rng: &mut Rng, true_percent: u64, false_percent: u64) -> Option<bool> {
    let roll = rng.below(100);
    if roll < true_percent {
        Some(true)
    } else if roll < true_percent + false_percent {
        Some(false)
    } else {
        None
    }
}

pub fn task_rows(count: u64, rng: &mut Rng) -> Vec<TaskRow> {
    let mut stamp = 2_000_000_000i64;
    (0..count)
        .map(|index| {
            if rng.below(100) >= 2 {
                stamp -= 1 + rng.below(3) as i64;
            }
            let header_len = 160 + rng.below(321);
            let header = rng.bytes(header_len);
            let status_len = 64 + rng.below(193);
            let status = Some(rng.bytes(status_len));
            TaskRow {
                id: task_id(index),
                stamp,
                session: Some(format!("s{}", rng.below(20))),
                observation: StoredObservation {
                    recorded_status: Some("running".into()),
                    active: flag(rng, 30, 60),
                    exited: flag(rng, 10, 60),
                    drained: flag(rng, 20, 60),
                    owned: flag(rng, 40, 50),
                },
                header,
                status,
            }
        })
        .collect()
}

fn valid_body(seq: u64, kind: &str, rng: &mut Rng) -> Vec<u8> {
    let head = format!(
        r#"{{"product":"harflow","format":"harflow.event","v":1,"product_version":"0.0.8","seq":{seq},"kind":"{kind}","observed_at":"2026-01-01T00:00:00Z","text":""#
    );
    let pad = rng.below(1001);
    let mut body = head.into_bytes();
    body.extend(rng.bytes(pad));
    body.extend_from_slice(b"\"}");
    body
}

pub fn event_records(count: u64, rng: &mut Rng) -> Vec<EventRecord> {
    let (mut start, mut seq) = (0u64, 0u64);
    (0..count)
        .map(|_| {
            let roll = rng.below(100);
            let (class, length, body, event_seq, kind);
            if roll < 2 {
                (class, length, body, event_seq, kind) =
                    (RecordClass::Blank, 1, Vec::new(), None, None);
            } else if roll < 4 {
                let len = rng.range(20, 200);
                (class, length, body, event_seq, kind) =
                    (RecordClass::Malformed, len, Vec::new(), None, None);
            } else {
                seq += 1;
                let name = KINDS[rng.below(10) as usize];
                let bytes = valid_body(seq, name, rng);
                (class, length, event_seq, kind) = (
                    RecordClass::Valid,
                    bytes.len() as u64 + 1,
                    Some(seq),
                    Some(name.to_owned()),
                );
                body = bytes;
            }
            let record = EventRecord {
                start,
                end: start + length,
                class,
                seq: event_seq,
                kind,
                body,
            };
            start += length;
            record
        })
        .collect()
}

pub fn facts() -> LogFacts {
    LogFacts {
        available: true,
        capture: CaptureState::Live,
        output_drained: None,
        eof_finished_seq: None,
        within_grace: false,
        task_status: Some("running".into()),
    }
}

/// Writes `tasks` tasks with `per_task` events each into a new store at `root`.
/// No checkpoint is run unless `checkpoint_every` is set: the `wal` state.
pub fn generate(
    root: &Path,
    tasks: u64,
    per_task: u64,
    checkpoint_every: Option<u64>,
    lease: Arc<dyn hf_application::lease::LeasePort>,
) -> Duration {
    let started = Instant::now();
    let deadline = || Instant::now() + Duration::from_secs(600);
    let store = Store::initialize_fixture(root, limits(), deadline(), lease).unwrap();
    let mut rng = Rng::new(SEED);
    let rows = task_rows(tasks, &mut rng);
    let coverage = Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    let fixture = TaskFixture {
        rows,
        pending: Vec::new(),
        coverage,
    };
    store.replace_task_fixture(&fixture, deadline()).unwrap();
    for index in 0..tasks {
        let fixture = EventFixture {
            task: task_id(index),
            records: event_records(per_task, &mut rng),
            partial_tail: 0,
            facts: facts(),
        };
        store.replace_event_fixture(&fixture, deadline()).unwrap();
        if checkpoint_every.is_some_and(|n| (index + 1) % n == 0) {
            store.checkpoint_passive(deadline()).unwrap();
        }
    }
    started.elapsed()
}
