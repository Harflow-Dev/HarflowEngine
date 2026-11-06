//! Experiments (a) to (c): SQLITE_OPEN_READ_ONLY against a WAL store.
use super::util::*;
use std::fs;

pub struct Scenario {
    pub name: &'static str,
    pub base: &'static str,
    pub remove: &'static [&'static str],
    pub ro_dir: bool,
    pub ro_files: bool,
    pub immutable: bool,
}

const WAL: &str = "store.sqlite3-wal";
const SHM: &str = "store.sqlite3-shm";

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "a1-wal+shm-writable-dir",
        base: "ci-wal",
        remove: &[],
        ro_dir: false,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "a2-wal+shm-readonly-dir",
        base: "ci-wal",
        remove: &[],
        ro_dir: true,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "a3-wal+shm-readonly-dir-and-files",
        base: "ci-wal",
        remove: &[],
        ro_dir: true,
        ro_files: true,
        immutable: false,
    },
    Scenario {
        name: "b1-wal-noshm-writable-dir",
        base: "ci-wal",
        remove: &[SHM],
        ro_dir: false,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "c1-wal-noshm-readonly-dir",
        base: "ci-wal",
        remove: &[SHM],
        ro_dir: true,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "c2-wal-noshm-readonly-dir-and-files",
        base: "ci-wal",
        remove: &[SHM],
        ro_dir: true,
        ro_files: true,
        immutable: false,
    },
    Scenario {
        name: "n1-immutable-control-wal-noshm",
        base: "ci-wal",
        remove: &[SHM],
        ro_dir: false,
        ro_files: false,
        immutable: true,
    },
    Scenario {
        name: "n2-immutable-control-wal+shm",
        base: "ci-wal",
        remove: &[],
        ro_dir: false,
        ro_files: false,
        immutable: true,
    },
    Scenario {
        name: "d1-nosidecars-writable-dir",
        base: "ci-ckpt",
        remove: &[WAL, SHM],
        ro_dir: false,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "d2-nosidecars-readonly-dir",
        base: "ci-ckpt",
        remove: &[WAL, SHM],
        ro_dir: true,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "e1-emptywal+shm-readonly-dir",
        base: "ci-ckpt",
        remove: &[],
        ro_dir: true,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "e2-emptywal-noshm-readonly-dir",
        base: "ci-ckpt",
        remove: &[SHM],
        ro_dir: true,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "f1-shm-only-lostwal-writable-dir",
        base: "ci-wal",
        remove: &[WAL],
        ro_dir: false,
        ro_files: false,
        immutable: false,
    },
    Scenario {
        name: "f2-lostwal-noshm-writable-dir",
        base: "ci-wal",
        remove: &[WAL, SHM],
        ro_dir: false,
        ro_files: false,
        immutable: false,
    },
];

pub fn run(sc: &Scenario) {
    let work = fresh(sc.name);
    let store = work.join("store");
    cp(&dir().join(sc.base), &store);
    for name in sc.remove {
        fs::remove_file(store.join(name)).unwrap();
    }
    let before = listing(&store);
    let wal_before = fs::read(store.join(WAL)).ok();
    lock_down(&store, sc.ro_dir, sc.ro_files);
    let db = store.join("store.sqlite3");
    let (result, wal_pragma) = match open_ro(&db, sc.immutable) {
        Ok(conn) => {
            let mode: Result<String, _> = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0));
            let ckpt: Result<(i64, i64, i64), _> =
                conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                });
            (counts(&conn), format!("{mode:?} ro_checkpoint={ckpt:?}"))
        }
        Err(e) => (format!("OPEN_ERROR {e}"), String::new()),
    };
    let after = listing(&store);
    let names_after = names(&store);
    unlock(&store);
    let wal_after = fs::read(store.join(WAL)).ok();
    let mut file = out("sidecars.jsonl");
    record(
        &mut file,
        &[
            ("scenario", sc.name.into()),
            ("expected", "events=30000 tasks=300".into()),
            ("result", result),
            ("journal_mode", wal_pragma),
            ("files_before", before),
            ("files_after", after),
            ("names_after", names_after),
            (
                "wal_bytes_unchanged",
                format!("{}", wal_before == wal_after),
            ),
        ],
    );
}
