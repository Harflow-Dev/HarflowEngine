//! Deterministic CI/large dataset generator over the existing fixture APIs.
use hf_application::{
    event_read::{EventRecord, RecordClass},
    task_read::TaskRow,
};
use hf_domain::timestamp::civil_from_days;
use hf_domain::{
    event_read::{CaptureState, LogFacts},
    task_read::{Coverage, StoredObservation, TaskId},
};
use hf_storage::{
    EventFixture, FixtureFacts, FixturePrompt, FixtureTask, Limits, Store, TaskFixture,
};
use std::{path::Path, sync::Arc, time::Duration, time::Instant};

pub const SEED: u64 = 0x4846_5332;
/// The stamp is a sort key in the legacy content; the valid content reads it as seconds and
/// shifts it into the past so no task starts after the measurement clock.
const STAMP_SHIFT: i64 = 600_000_000;
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

/// How task header and status documents are produced.
/// `Legacy` is the S2 content (random alphabetic bytes, written with `replace_task_fixture`),
/// kept so S2 results stay reproducible. `Valid` is the S6 content: JSON documents served by
/// the public reads, written with `add_task_fixture`. Both consume the same seeded draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskBytes {
    Legacy,
    Valid,
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

fn clock(seconds: i64) -> String {
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let rest = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}+00:00",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// A JSON object of exactly `total` bytes: `head` (the keys, without braces) plus a `pad`
/// string member filled from the seeded `filler` bytes.
fn padded(head: &str, filler: &[u8], total: u64) -> Vec<u8> {
    let base = format!("{{{head},\"pad\":\"\"}}");
    let room = (total as usize).saturating_sub(base.len());
    let mut doc = format!("{{{head},\"pad\":\"").into_bytes();
    doc.extend_from_slice(&filler[..room.min(filler.len())]);
    doc.extend_from_slice(b"\"}");
    doc
}

/// Valid tasks for the S6 content. The seeded draws are exactly those of `task_rows`, so the
/// event content that follows in the same stream is unchanged. Status is derived from the
/// `active` draw: true is `running` without an exit code, false is `completed` with exit 0,
/// unknown (not representable by `add_task_fixture`) is `failed` with exit 1. The `exited`
/// and `owned` draws are consumed and unused: exit facts follow status, and E1 has no owner
/// facts.
pub fn valid_tasks(count: u64, rng: &mut Rng) -> Vec<FixtureTask> {
    let mut stamp = 2_000_000_000i64;
    (0..count)
        .map(|index| {
            if rng.below(100) >= 2 {
                stamp -= 1 + rng.below(3) as i64;
            }
            let header_len = 160 + rng.below(321);
            let header_fill = rng.bytes(header_len);
            let status_len = 64 + rng.below(193);
            let status_fill = rng.bytes(status_len);
            let session = format!("s{}", rng.below(20));
            let active = flag(rng, 30, 60);
            let _exited = flag(rng, 10, 60);
            let drained = flag(rng, 20, 60);
            let _owned = flag(rng, 40, 50);
            let id = task_id(index);
            let (status, exit_code) = match active {
                Some(true) => ("running", None),
                Some(false) => ("completed", Some(0)),
                None => ("failed", Some(1)),
            };
            let started = clock(stamp - STAMP_SHIFT);
            let finished = exit_code.map(|_| clock(stamp - STAMP_SHIFT + 60));
            let header = format!(
                r#""task_id":"{}","backend":"claude","session_id":"{session}","started_at":"{started}""#,
                id.as_str()
            );
            let status_head = format!(r#""task_id":"{}","status":"{status}""#, id.as_str());
            FixtureTask {
                facts: FixtureFacts {
                    id: id.clone(),
                    backend: "claude".into(),
                    repo_path: "/workspace/repo".into(),
                    session_id: session,
                    status: status.into(),
                    exit_code,
                    started_at: started,
                    started_at_us: (stamp - STAMP_SHIFT) * 1_000_000,
                    finished_at: finished,
                    finished_at_us: exit_code.map(|_| (stamp - STAMP_SHIFT + 60) * 1_000_000),
                    output_drained: drained == Some(true),
                    title: None,
                    model: None,
                    continuation_of: None,
                    chain_root: None,
                },
                header: padded(&header, &header_fill, header_len),
                status: padded(&status_head, &status_fill, status_len),
                prompt: FixturePrompt {
                    inline_text: Some("Summarize the repository.".into()),
                    preview: None,
                    claimed_sha256: None,
                    source_name: None,
                    blob: None,
                },
                process: None,
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
    bytes: TaskBytes,
    lease: Arc<dyn hf_application::lease::LeasePort>,
) -> Duration {
    let started = Instant::now();
    let deadline = || Instant::now() + Duration::from_secs(600);
    let store = Store::initialize_fixture(root, limits(), deadline(), lease).unwrap();
    let mut rng = Rng::new(SEED);
    let coverage = Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    let empty = |rows| TaskFixture {
        rows,
        pending: Vec::new(),
        coverage,
    };
    match bytes {
        TaskBytes::Legacy => {
            let rows = task_rows(tasks, &mut rng);
            store
                .replace_task_fixture(&empty(rows), deadline())
                .unwrap();
        }
        TaskBytes::Valid => {
            store
                .replace_task_fixture(&empty(Vec::new()), deadline())
                .unwrap();
            for task in valid_tasks(tasks, &mut rng) {
                store.add_task_fixture(&task, deadline()).unwrap();
            }
        }
    }
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
