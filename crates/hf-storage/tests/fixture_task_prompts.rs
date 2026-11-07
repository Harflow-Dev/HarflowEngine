#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The process row and the prompt rows of `add_task_fixture`, and the prompt blob writer.
mod common;
mod fixture_support;
use fixture_support::{A, B, count, done, hex, identity, later, quoted, task, tid, world};

const PROC: &str = "pid,start_seconds,start_micros,boot_uuid,pgid";
use hf_storage::{FixtureProcess, FixturePrompt};

#[test]
fn a_process_row_with_a_decided_identity_stores_the_start_columns_and_no_group() {
    let w = world();
    let mut t = task(A, "running", None, false);
    t.process = Some(FixtureProcess {
        pid: 4242,
        start: Some(identity(4242)),
    });
    w.add(&t);
    assert_eq!(
        quoted(&w.db(), "task_processes", PROC, "task", A),
        "4242|1768000000|123456|'01234567-89ab-cdef-0123-456789abcdef'|NULL"
    );
}

#[test]
fn an_undecided_identity_stores_null_start_columns_together() {
    let w = world();
    let mut t = task(A, "running", None, false);
    t.process = Some(FixtureProcess {
        pid: 4242,
        start: None,
    });
    w.add(&t);
    assert_eq!(
        quoted(&w.db(), "task_processes", PROC, "task", A),
        "4242|NULL|NULL|NULL|NULL"
    );
    w.add(&done(B));
    assert_eq!(
        count(&w.db(), "task_processes", "task", B),
        0,
        "no process, no row"
    );
}

const PROMPT: &str = "form,inline_text,preview,claimed_sha256,source_name,truncated,blob";

fn prompt_row(w: &fixture_support::World, id: &str) -> String {
    quoted(&w.db(), "task_prompts", PROMPT, "task", id)
}

fn sidecar(blob: Option<&[u8]>) -> FixturePrompt {
    FixturePrompt {
        inline_text: None,
        preview: Some("line 0000".into()),
        claimed_sha256: Some(
            "5ad38304b535c2987dbd24657c1a11b884984ff600d9f389deb0d4e634fee792".into(),
        ),
        source_name: Some("name.txt".into()),
        blob: blob.map(<[u8]>::to_vec),
    }
}

#[test]
fn inline_and_sidecar_prompts_are_stored_by_form() {
    let w = world();
    w.add(&done(A));
    let mut t = done(B);
    t.prompt = sidecar(Some(b"full text"));
    w.add(&t);
    assert_eq!(
        prompt_row(&w, A),
        "'inline'|'Summarize the repo.'|NULL|NULL|NULL|0|NULL"
    );
    assert_eq!(
        prompt_row(&w, B),
        format!(
            "'sidecar'|NULL|'line 0000'|'5ad38304b535c2987dbd24657c1a11b884984ff600d9f389deb0d4e634fee792'|'name.txt'|1|{}",
            hex(b"full text")
        )
    );
}

#[test]
fn a_sidecar_without_a_blob_or_a_digest_keeps_those_columns_null() {
    let w = world();
    let mut t = done(A);
    t.prompt = FixturePrompt {
        claimed_sha256: None,
        ..sidecar(None)
    };
    w.add(&t);
    assert_eq!(
        prompt_row(&w, A),
        "'sidecar'|NULL|'line 0000'|NULL|'name.txt'|1|NULL"
    );
}

#[test]
fn appending_to_a_stored_prompt_changes_only_the_blob() {
    let w = world();
    let mut t = done(A);
    t.prompt = sidecar(Some(b"full text"));
    w.add(&t);
    assert!(
        w.store
            .append_prompt_blob_fixture(&tid(A), b"tampered", later(5))
            .unwrap()
    );
    assert_eq!(
        prompt_row(&w, A),
        format!(
            "'sidecar'|NULL|'line 0000'|'5ad38304b535c2987dbd24657c1a11b884984ff600d9f389deb0d4e634fee792'|'name.txt'|1|{}",
            hex(b"full texttampered")
        )
    );
    assert!(
        w.store
            .append_prompt_blob_fixture(&tid(A), b"!", later(5))
            .unwrap()
    );
    assert_eq!(
        quoted(&w.db(), "task_prompts", "blob", "task", A),
        hex(b"full texttampered!")
    );
}

#[test]
fn appending_without_a_stored_prompt_changes_nothing_and_says_so() {
    let w = world();
    w.add(&done(A));
    let mut missing = done(B);
    missing.prompt = sidecar(None);
    w.add(&missing);
    let inline_before = prompt_row(&w, A);
    let missing_before = prompt_row(&w, B);
    assert!(
        !w.store
            .append_prompt_blob_fixture(&tid(A), b"x", later(5))
            .unwrap()
    );
    assert!(
        !w.store
            .append_prompt_blob_fixture(&tid(B), b"x", later(5))
            .unwrap()
    );
    assert!(
        !w.store
            .append_prompt_blob_fixture(&tid(fixture_support::C), b"x", later(5))
            .unwrap()
    );
    assert_eq!(prompt_row(&w, A), inline_before);
    assert_eq!(prompt_row(&w, B), missing_before);
}
