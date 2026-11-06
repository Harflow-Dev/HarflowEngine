//! Shared helpers for the S2 read-path experiments.
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

pub fn dir() -> PathBuf {
    PathBuf::from(std::env::var("HF_MEASURE_DIR").expect("set HF_MEASURE_DIR"))
}

pub fn out(name: &str) -> File {
    let out = PathBuf::from(std::env::var("HF_MEASURE_OUT").expect("set HF_MEASURE_OUT"));
    fs::create_dir_all(&out).unwrap();
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join(name))
        .unwrap()
}

pub fn cp(from: &Path, to: &Path) {
    assert!(
        Command::new("cp")
            .arg("-cR")
            .arg(from)
            .arg(to)
            .status()
            .unwrap()
            .success()
    );
}

pub fn fresh(name: &str) -> PathBuf {
    // The measurement tree has inherited ACLs that override read-only mode bits, so
    // experiments run in HF_EXP_WORK, a directory without them.
    let base = std::env::var("HF_EXP_WORK").map_or_else(|_| dir().join("exp"), PathBuf::from);
    let path = base.join(name);
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

pub fn listing(dir: &Path) -> String {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            let mtime = m
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            format!(
                "{}:{}:{}",
                e.file_name().to_string_lossy(),
                m.len(),
                mtime.as_nanos()
            )
        })
        .collect();
    names.sort();
    names.join(";")
}

pub fn names(dir: &Path) -> String {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.join(",")
}

pub fn lock_down(dir: &Path, directory: bool, files: bool) {
    if files {
        for e in fs::read_dir(dir).unwrap() {
            fs::set_permissions(e.unwrap().path(), fs::Permissions::from_mode(0o444)).unwrap();
        }
    }
    if directory {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
    }
}

pub fn unlock(dir: &Path) {
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
    for e in fs::read_dir(dir).unwrap() {
        fs::set_permissions(e.unwrap().path(), fs::Permissions::from_mode(0o644)).unwrap();
    }
}

pub fn short(error: &rusqlite::Error) -> String {
    json(&format!("{error}"))
}

pub fn json(text: &str) -> String {
    text.replace('\\', "/").replace('"', "'").replace('\n', " ")
}

pub fn open_ro(path: &Path, immutable: bool) -> Result<Connection, String> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let uri = format!(
        "file:{}?{}",
        path.display(),
        if immutable { "immutable=1" } else { "mode=ro" }
    );
    Connection::open_with_flags(uri, flags).map_err(|e| short(&e))
}

/// Logical content probe: (events, tasks) or the first error.
pub fn counts(conn: &Connection) -> String {
    let query = "SELECT (SELECT count(*) FROM event_records),(SELECT count(*) FROM task_headers)";
    match conn.query_row(query, [], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
    }) {
        Ok((events, tasks)) => format!("events={events} tasks={tasks}"),
        Err(e) => format!("ERROR {}", short(&e)),
    }
}

pub fn record(file: &mut File, fields: &[(&str, String)]) {
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!(r#""{k}":"{}""#, json(v)))
        .collect();
    writeln!(file, "{{{}}}", body.join(",")).unwrap();
}
