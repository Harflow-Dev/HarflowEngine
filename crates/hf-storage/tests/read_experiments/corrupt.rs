//! Experiment 4: what a read does when bytes are damaged.
use super::{data::Rng, ops, util::*};
use hf_application::event_read::{EventRecord, RecordClass};
use rusqlite::Connection;
use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
};

const ERR: u64 = u64::MAX;
const PAGE: u64 = 4096;
const FRAME: u64 = 24 + PAGE;

pub struct Probe {
    pub open: String,
    pub events: String,
    pub hashes: Vec<u64>,
    pub quick: String,
}

fn mix(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h = (*h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
    }
}

fn class_no(class: RecordClass) -> i64 {
    match class {
        RecordClass::Valid => 0,
        RecordClass::Blank => 1,
        RecordClass::Malformed => 2,
        RecordClass::Oversized => 3,
    }
}

/// Hash of every materialized column of one record, in the stored representation.
fn mix_record(h: &mut u64, row: (i64, i64, i64, Option<i64>, Option<String>, Vec<u8>)) {
    for n in [row.0, row.1, row.2, row.3.unwrap_or(-1)] {
        mix(h, &n.to_le_bytes());
    }
    mix(h, row.4.as_deref().unwrap_or("\0none").as_bytes());
    mix(h, &row.5);
}

fn page_hash(conn: &Connection, task: u64) -> u64 {
    let sql = "SELECT start,end,class,seq,kind,body FROM event_records WHERE task=?1 ORDER BY start DESC LIMIT 50";
    let id = format!("t-{task:08x}");
    let run = || -> rusqlite::Result<u64> {
        let mut st = conn.prepare(sql)?;
        let mut rows = st.query([id.as_str()])?;
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        while let Some(r) = rows.next()? {
            let row = (
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            );
            mix_record(&mut h, row);
        }
        Ok(h)
    };
    run().unwrap_or(ERR)
}

/// Independent baseline: regenerated from the generator, never read from a store.
pub fn generated() -> Vec<Vec<EventRecord>> {
    let mut rng = Rng::new(super::data::SEED);
    let _ = super::data::task_rows(300, &mut rng);
    (0..300)
        .map(|_| super::data::event_records(100, &mut rng))
        .collect()
}

pub fn baseline(generated: &[Vec<EventRecord>]) -> Probe {
    let hashes = generated
        .iter()
        .map(|records| {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for r in records.iter().rev().take(50) {
                let row = (
                    r.start as i64,
                    r.end as i64,
                    class_no(r.class),
                    r.seq.map(|s| s as i64),
                    r.kind.clone(),
                    r.body.clone(),
                );
                mix_record(&mut h, row);
            }
            h
        })
        .collect();
    let total: usize = generated.iter().map(Vec::len).sum();
    let bytes: usize = generated.iter().flatten().map(|r| r.body.len()).sum();
    Probe {
        open: String::new(),
        events: format!("{total}/{bytes}"),
        hashes,
        quick: "ok".into(),
    }
}

pub fn probe(db: &Path) -> Probe {
    let conn = match open_ro(db, false) {
        Ok(c) => c,
        Err(e) => {
            return Probe {
                open: e,
                events: String::new(),
                hashes: vec![],
                quick: String::new(),
            };
        }
    };
    let events = match conn.query_row(
        "SELECT count(*),coalesce(sum(length(body)),0) FROM event_records",
        [],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ) {
        Ok((n, b)) => format!("{n}/{b}"),
        Err(e) => format!("ERROR {}", short(&e)),
    };
    let hashes = (0..300).map(|t| page_hash(&conn, t)).collect();
    let quick = conn
        .query_row("PRAGMA quick_check(1)", [], |r| r.get::<_, String>(0))
        .unwrap_or_else(|e| format!("ERROR {}", short(&e)));
    Probe {
        open: String::new(),
        events,
        hashes,
        quick,
    }
}

/// refused (error seen by a read), silent_wrong (differs, no error), unaffected.
fn classify(p: &Probe, truth: &Probe) -> &'static str {
    let errors = !p.open.is_empty() || p.events.starts_with("ERROR") || p.hashes.contains(&ERR);
    if errors {
        "refused"
    } else if p.events != truth.events || p.hashes != truth.hashes {
        "silent_wrong"
    } else {
        "unaffected"
    }
}

pub fn write_at(path: &Path, offset: u64, bytes: &[u8]) {
    let mut f = OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(offset)).unwrap();
    f.write_all(bytes).unwrap();
}

fn flip(path: &Path, offset: u64) {
    let mut byte = [0u8; 1];
    let data = fs::read(path).unwrap();
    byte[0] = data[offset as usize] ^ 0x5a;
    write_at(path, offset, &byte);
}

fn trial(
    base: &str,
    name: &str,
    drop_shm: bool,
    damage: impl Fn(&Path),
) -> (Probe, std::path::PathBuf) {
    let work = fresh(name);
    let store = work.join("store");
    cp(&dir().join(base), &store);
    if drop_shm {
        fs::remove_file(store.join("store.sqlite3-shm")).unwrap();
    }
    damage(&store);
    (probe(&store.join("store.sqlite3")), store)
}

fn emit(kind: &str, detail: &str, p: &Probe, truth: &Probe, old: &str) {
    let mut file = out("corruption.jsonl");
    let diff = p
        .hashes
        .iter()
        .zip(&truth.hashes)
        .filter(|(a, b)| a != b)
        .count();
    record(
        &mut file,
        &[
            ("kind", kind.into()),
            ("detail", detail.into()),
            ("outcome", classify(p, truth).into()),
            ("open", p.open.clone()),
            ("events_count_bytes", p.events.clone()),
            ("tasks_with_different_page", diff.to_string()),
            ("quick_check", p.quick.clone()),
            ("old_path", old.into()),
        ],
    );
}

fn old_path(store: &Path) -> String {
    let root = store.to_path_buf();
    match ops::old_open(&root) {
        Err(e) => format!("open refused: {e}"),
        Ok(s) => {
            let g = generated();
            let n = |t: u64| match ops::old_events(&s, &super::data::task_id(t)) {
                Ok(v) => format!(
                    "Ok({} records, all equal to generated: {})",
                    v.len(),
                    v.iter().all(|r| g[t as usize].contains(r))
                ),
                Err(e) => format!("Err({e})"),
            };
            format!(
                "open ok; events(t299)={:?} events(t150)={:?}",
                n(299),
                n(150)
            )
        }
    }
}

pub fn main_file() {
    let truth = baseline(&generated());
    let pristine = trial("ci-ckpt", "corrupt-truth-main", false, |_| {}).0;
    let mut file = out("corruption.jsonl");
    record(
        &mut file,
        &[
            ("kind", "baseline_check".into()),
            (
                "pristine_matches_generated",
                (classify(&pristine, &truth) == "unaffected").to_string(),
            ),
        ],
    );
    let size = fs::metadata(dir().join("ci-ckpt/store.sqlite3"))
        .unwrap()
        .len();
    let pages = size / PAGE;
    let mut rng = Rng::new(7);
    for kind in ["zero_page", "flip_byte"] {
        for n in 0..20 {
            let page = 2 + rng.below(pages - 2);
            let at = rng.below(PAGE);
            let (p, store) = trial("ci-ckpt", "corrupt-main", false, |s| {
                let db = s.join("store.sqlite3");
                if kind == "zero_page" {
                    write_at(&db, (page - 1) * PAGE, &[0u8; PAGE as usize]);
                } else {
                    flip(&db, (page - 1) * PAGE + at);
                }
            });
            let old = if n < 4 {
                old_path(&store)
            } else {
                String::new()
            };
            emit(kind, &format!("page={page} offset={at}"), &p, &truth, &old);
        }
    }
    for (kind, apply) in [
        ("zero_header_page", 0u8),
        ("truncate_half", 1),
        ("truncate_tail_100", 2),
    ] {
        let (p, store) = trial("ci-ckpt", "corrupt-main", false, |s| {
            let db = s.join("store.sqlite3");
            match apply {
                0 => write_at(&db, 0, &[0u8; PAGE as usize]),
                1 => OpenOptions::new()
                    .write(true)
                    .open(&db)
                    .unwrap()
                    .set_len(size / 2)
                    .unwrap(),
                _ => OpenOptions::new()
                    .write(true)
                    .open(&db)
                    .unwrap()
                    .set_len(size - 100)
                    .unwrap(),
            }
        });
        emit(kind, "", &p, &truth, &old_path(&store));
    }
}

type Damage = Box<dyn Fn(&Path)>;

fn wal_cases(size: u64, frames: u64) -> Vec<(&'static str, bool, Damage)> {
    let at = move |pct: u64| 32 + (frames * pct / 100) * FRAME;
    vec![
        (
            "flip_page_byte_50pct_noshm",
            true,
            Box::new(move |s| flip(&s.join("store.sqlite3-wal"), at(50) + 24 + 100)),
        ),
        (
            "flip_page_byte_50pct_shmkept",
            false,
            Box::new(move |s| flip(&s.join("store.sqlite3-wal"), at(50) + 24 + 100)),
        ),
        (
            "flip_page_byte_last_frame_noshm",
            true,
            Box::new(move |s| flip(&s.join("store.sqlite3-wal"), at(100) - FRAME + 24 + 100)),
        ),
        (
            "flip_page_byte_last_frame_shmkept",
            false,
            Box::new(move |s| flip(&s.join("store.sqlite3-wal"), at(100) - FRAME + 24 + 100)),
        ),
        (
            "zero_last_frame_header_noshm",
            true,
            Box::new(move |s| write_at(&s.join("store.sqlite3-wal"), at(100) - FRAME, &[0u8; 24])),
        ),
        (
            "truncate_mid_frame_1000_noshm",
            true,
            Box::new(move |s| {
                OpenOptions::new()
                    .write(true)
                    .open(s.join("store.sqlite3-wal"))
                    .unwrap()
                    .set_len(size - 1000)
                    .unwrap()
            }),
        ),
        (
            "truncate_to_header_noshm",
            true,
            Box::new(|s| {
                OpenOptions::new()
                    .write(true)
                    .open(s.join("store.sqlite3-wal"))
                    .unwrap()
                    .set_len(32)
                    .unwrap()
            }),
        ),
        (
            "corrupt_wal_magic_noshm",
            true,
            Box::new(|s| flip(&s.join("store.sqlite3-wal"), 0)),
        ),
        (
            "flip_frame_salt_50pct_noshm",
            true,
            Box::new(move |s| flip(&s.join("store.sqlite3-wal"), at(50) + 8)),
        ),
    ]
}

pub fn wal_file() {
    let truth = baseline(&generated());
    let pristine = trial("ci-wal", "corrupt-truth-wal", true, |_| {}).0;
    let mut file = out("corruption.jsonl");
    record(
        &mut file,
        &[
            ("kind", "baseline_check_wal".into()),
            (
                "pristine_matches_generated",
                (classify(&pristine, &truth) == "unaffected").to_string(),
            ),
        ],
    );
    let size = fs::metadata(dir().join("ci-wal/store.sqlite3-wal"))
        .unwrap()
        .len();
    let frames = (size - 32) / FRAME;
    let cases = wal_cases(size, frames);
    for (kind, drop_shm, damage) in cases {
        let (p, store) = trial("ci-wal", "corrupt-wal", drop_shm, damage);
        emit(
            kind,
            &format!("frames={frames}"),
            &p,
            &truth,
            &old_path(&store),
        );
    }
}
