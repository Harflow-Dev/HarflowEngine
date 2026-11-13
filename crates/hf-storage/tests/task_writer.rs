#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `Store::create_task`: one atomic publication, size edges, the inline and sidecar prompt
//! forms, the 4 MiB prompt cap, parity with the fixture wrapper and existing-id refusals.
mod common;
mod fixture_support;
mod task_writer_support;
use fixture_support::{A, B, World, is_hex32, later, one, tid, world};
use hf_domain::{
    ctl::history::{PromptBlob, StoredPrompt, sidecar_digest},
    process::ProcessIdentity,
    task_read::{AuthorizedScope, Lookup, TaskQuery},
};
use hf_storage::{Error, FixturePrompt, FixtureTask, TaskCreate, TaskProcess};
use task_writer_support::{create, done, process, published, put, rows};

/// sha256 of one million "a" bytes, a published test vector.
const MILLION_A: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
const FOUR_MIB: usize = 4 * 1024 * 1024;

fn stored_prompt(w: &World, id: &str) -> StoredPrompt {
    w.session()
        .snapshot(later(30), |s| s.prompt(&tid(id), 10))
        .unwrap()
        .unwrap()
}

fn refused(w: &World, task: &TaskCreate) -> Error {
    let before = published(w);
    let error = w.store.create_task(task, later(5)).unwrap_err();
    assert_eq!(published(w), before, "a refused create publishes nothing");
    error
}

#[test]
fn a_created_task_lands_with_its_rows_generation_revision_and_sequence_together() {
    let w = world();
    let (generation, watch, sequence) = published(&w);
    let mut t = done(A);
    t.process = Some(process(4242));
    put(&w, &t);
    let after = published(&w);
    assert!(is_hex32(&after.0), "{}", after.0);
    assert_ne!(after.0, generation);
    assert_eq!((after.1, after.2), (watch + 1, sequence + 1));
    assert_eq!(
        rows(&w, A),
        [
            "1|'claude'|'/workspace/repo'|'session-aaaaaaaa'|'completed'|0|'2026-01-10T10:00:00+00:00'|1768039200000000|'2026-01-10T10:05:00+00:00'|1768039500000000|1|'First task'|NULL|NULL|NULL".to_owned(),
            format!(
                "1768039200000000|'session-aaaaaaaa'|0|'completed'|1|1|0|{}|{}",
                fixture_support::hex(format!("header:{A}").as_bytes()),
                fixture_support::hex(format!("status:{A}").as_bytes())
            ),
            "'inline'|'Summarize the repo.'|NULL|NULL|NULL|0|NULL".to_owned(),
            "4242|1768000000|123456|'01234567-89ab-cdef-0123-456789abcdef'|NULL".to_owned(),
        ]
    );
    let found = w
        .session()
        .task_read(
            &AuthorizedScope::Global,
            &TaskQuery::Ids(vec![tid(A)]),
            later(5),
        )
        .unwrap();
    assert_eq!(found.lookups, [Lookup::Found(tid(A))]);
}

/// No stage hook fires on this path, so a pre-commit kill cannot be staged from here; an
/// orphan process row for the id, written past the writer, makes the last insert of the
/// transaction fail after the facts, header and prompt rows are in.
#[test]
fn a_failure_on_the_last_row_rolls_back_every_row_and_publishes_nothing() {
    let w = world();
    w.raw()
        .execute(
            "INSERT INTO task_processes VALUES(?1,4243,NULL,NULL,NULL,NULL)",
            [A],
        )
        .unwrap();
    let mut t = done(A);
    t.process = Some(process(4242));
    let error = refused(&w, &t);
    assert!(
        matches!(
            &error,
            Error::Sqlite(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation
        ),
        "{error:?}"
    );
    assert_eq!(
        rows(&w, A),
        ["NONE", "NONE", "NONE", "4243|NULL|NULL|NULL|NULL"]
    );
    put(&w, &done(A));
}

/// Start seconds that do not fit a SQLite integer are refused before the transaction.
#[test]
fn a_process_start_beyond_i64_is_refused_as_a_limit_and_publishes_nothing() {
    let w = world();
    let mut t = done(A);
    let start = ProcessIdentity::new(
        4242,
        i64::MAX as u64 + 1,
        1,
        "01234567-89ab-cdef-0123-456789abcdef".into(),
    )
    .unwrap();
    t.process = Some(TaskProcess {
        pid: 4242,
        start: Some(start),
    });
    assert!(matches!(refused(&w, &t), Error::LimitExceeded));
    assert_eq!(rows(&w, A), ["NONE", "NONE", "NONE", "NONE"]);
    let mut fits = done(A);
    fits.process = Some(TaskProcess {
        pid: 4242,
        start: Some(
            ProcessIdentity::new(
                4242,
                i64::MAX as u64,
                1,
                "01234567-89ab-cdef-0123-456789abcdef".into(),
            )
            .unwrap(),
        ),
    });
    put(&w, &fits);
}

#[test]
fn header_and_status_blobs_of_2048_bytes_land_and_2049_are_refused() {
    let w = world();
    let mut t = done(A);
    t.header = vec![b'h'; 2048];
    t.status = vec![b's'; 2048];
    put(&w, &t);
    let len: i64 = one(
        &w.db(),
        "SELECT length(header)+length(status) FROM task_headers WHERE id=?1",
        [A],
    );
    assert_eq!(len, 4096);
    for header in [true, false] {
        let mut t = done(B);
        *(if header { &mut t.header } else { &mut t.status }) = vec![b'x'; 2049];
        assert!(matches!(refused(&w, &t), Error::LimitExceeded));
    }
    assert_eq!(rows(&w, B), ["NONE", "NONE", "NONE", "NONE"]);
}

fn prompt_cell<T: rusqlite::types::FromSql>(w: &World, id: &str, expression: &str) -> T {
    one(
        &w.db(),
        &format!("SELECT {expression} FROM task_prompts WHERE task=?1"),
        [id],
    )
}

#[test]
fn a_prompt_of_2000_characters_is_inline_whole_including_multibyte_text() {
    for prompt in [
        "x".repeat(2000),
        format!("a{}", "\u{65e5}".repeat(1999)),
        "\u{1f600}".repeat(2000),
    ] {
        let w = world();
        put(&w, &create(A, "completed", Some(0), &prompt));
        let form: String = prompt_cell(&w, A, "form");
        let text: String = prompt_cell(&w, A, "inline_text");
        assert_eq!((form.as_str(), text), ("inline", prompt));
        assert_eq!(stored_prompt(&w, A).blob, PromptBlob::Absent);
    }
}

#[test]
fn a_prompt_of_2001_characters_previews_exactly_the_first_2000_on_a_character_boundary() {
    for (prompt, preview) in [
        ("x".repeat(2001), "x".repeat(2000)),
        (
            format!("a{}", "\u{65e5}".repeat(2000)),
            format!("a{}", "\u{65e5}".repeat(1999)),
        ),
        ("\u{1f600}".repeat(2001), "\u{1f600}".repeat(2000)),
    ] {
        let w = world();
        put(&w, &create(A, "completed", Some(0), &prompt));
        let stored: String = prompt_cell(&w, A, "preview");
        assert_eq!(stored, preview);
        let chars: i64 = prompt_cell(&w, A, "length(preview)");
        let bytes: i64 = prompt_cell(&w, A, "length(CAST(preview AS BLOB))");
        assert_eq!((chars, bytes), (2000, preview.len() as i64));
        let read = stored_prompt(&w, A);
        assert_eq!(read.preview.as_deref(), Some(preview.as_str()));
        assert_eq!(read.inline, None);
        assert_eq!(read.blob, PromptBlob::Verified(prompt.into_bytes()));
    }
}

#[test]
fn a_sidecar_is_named_and_claimed_by_its_digest_and_reads_back_verified() {
    let w = world();
    let prompt = "a".repeat(1_000_000);
    put(&w, &create(A, "completed", Some(0), &prompt));
    let name = format!("{A}.prompt.{MILLION_A}.txt");
    assert_eq!(
        fixture_support::quoted(
            &w.db(),
            "task_prompts",
            "form,inline_text,claimed_sha256,source_name,truncated,length(blob)",
            "task",
            A
        ),
        format!("'sidecar'|NULL|'{MILLION_A}'|'{name}'|1|1000000")
    );
    assert_eq!(sidecar_digest(A, &name), Some(MILLION_A));
    let read = stored_prompt(&w, A);
    assert!(read.bound);
    assert_eq!(read.title.as_deref(), Some("First task"));
    assert_eq!(read.preview, Some("a".repeat(2000)));
    assert_eq!(read.blob, PromptBlob::Verified(prompt.into_bytes()));
}

#[test]
fn a_prompt_of_exactly_4_mib_is_stored_and_verifiable() {
    let w = world();
    let prompt = "a".repeat(FOUR_MIB);
    put(&w, &create(A, "completed", Some(0), &prompt));
    let len: i64 = prompt_cell(&w, A, "length(blob)");
    assert_eq!(len, FOUR_MIB as i64);
    assert_eq!(
        stored_prompt(&w, A).blob,
        PromptBlob::Verified(prompt.into_bytes())
    );
}

#[test]
fn a_prompt_one_byte_over_4_mib_is_refused_counting_bytes_not_characters() {
    let w = world();
    for prompt in ["a".repeat(FOUR_MIB + 1), "\u{e9}".repeat(FOUR_MIB / 2 + 1)] {
        let error = refused(&w, &create(A, "completed", Some(0), &prompt));
        assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    }
    assert_eq!(rows(&w, A), ["NONE", "NONE", "NONE", "NONE"]);
}

fn as_fixture(task: &TaskCreate, prompt: FixturePrompt) -> FixtureTask {
    FixtureTask {
        facts: task.facts.clone(),
        header: task.header.clone(),
        status: task.status.clone(),
        prompt,
        process: task.process.clone(),
    }
}

#[test]
fn the_fixture_wrapper_and_create_task_store_identical_rows() {
    let long = "a".repeat(1_000_000);
    let sidecar = FixturePrompt {
        inline_text: None,
        preview: Some("a".repeat(2000)),
        claimed_sha256: Some(MILLION_A.into()),
        source_name: Some(format!("{A}.prompt.{MILLION_A}.txt")),
        blob: Some(long.clone().into_bytes()),
    };
    let inline = fixture_support::inline_prompt("Summarize the repo.");
    for (text, prompt) in [("Summarize the repo.", inline), (long.as_str(), sidecar)] {
        let mut direct = create(A, "completed", Some(0), text);
        direct.process = Some(process(4242));
        let (production, wrapped) = (world(), world());
        put(&production, &direct);
        wrapped.add(&as_fixture(&direct, prompt));
        assert_eq!(rows(&production, A), rows(&wrapped, A));
        assert_eq!(stored_prompt(&production, A), stored_prompt(&wrapped, A));
    }
}

#[test]
fn an_existing_valid_or_damaged_id_is_refused_by_create_and_by_the_wrapper() {
    let w = world();
    put(&w, &done(A));
    w.store.damage_task_fixture(&tid(B), later(5)).unwrap();
    let kept = (rows(&w, A), rows(&w, B));
    for id in [A, B] {
        let other = create(id, "failed", Some(1), &"z".repeat(2001));
        let error = refused(&w, &other);
        assert!(matches!(error, Error::TaskExists), "{id}: {error:?}");
        let before = published(&w);
        let error = w
            .store
            .add_task_fixture(
                &as_fixture(&other, fixture_support::inline_prompt("z")),
                later(5),
            )
            .unwrap_err();
        assert!(matches!(error, Error::FixtureConflict), "{id}: {error:?}");
        assert_eq!(published(&w), before);
    }
    assert_eq!((rows(&w, A), rows(&w, B)), kept);
}
