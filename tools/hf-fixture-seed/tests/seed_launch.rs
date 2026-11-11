#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Launch receipts through the real process: what is stored, and how the ordinary reads see
//! it. Stored columns are read with the system `sqlite3`.
mod support;
use hf_domain::task_read::Lookup;
use std::path::Path;
use support::{A, B, empty, esc, lookup, ok, query, run, serial};

const NAME: &str = "11111111-1111-4111-8111-111111111111";
const GENERATION: &str = "cccccccccccccccccccccccccccccccc";

/// An `add_launch` request; `over` entries hold raw JSON and win over the defaults.
fn launch(name: &str, over: &[(&str, &str)]) -> String {
    let mut fields = vec![
        ("name", esc(name)),
        ("state", esc("waiting_for_resources")),
        ("generation", esc(GENERATION)),
        ("backend", esc("claude")),
        ("title", esc("t")),
        ("repo_path", esc("/r")),
        ("created_at", esc("2026-11-10T00:00:00+00:00")),
        ("not_started_reason", "null".into()),
        ("lineage", "{}".into()),
        ("resources", r#"{"names":["fixture"]}"#.into()),
        ("worktree", "null".into()),
        ("refusal", "null".into()),
    ];
    for (key, value) in over {
        fields.iter_mut().find(|(k, _)| k == key).unwrap().1 = (*value).to_owned();
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{}:{v}", esc(k)))
        .collect();
    format!("{{\"spec\":{{{}}}}}", body.join(","))
}

const SHAPE: &str = "quote(state)||'|'||quote(has_resources)||'|'||quote(has_worktree)||'|'||quote(refusal_code)||'|'||quote(refusal_check)||'|'||quote(title)||'|'||quote(backend)||'|'||quote(generation)||'|'||quote(resource_names)";

fn stored(root: &Path, name: &str) -> String {
    query(
        root,
        &format!("SELECT {SHAPE} FROM launch_attempts WHERE name='{name}'"),
    )
}

#[test]
fn every_refusal_check_maps_to_its_stored_code_and_check() {
    let _serial = serial();
    let scratch = empty();
    for check in [
        "format",
        "version",
        "root_mismatch",
        "task_id_mismatch",
        "generation",
        "oversize",
        "field_shape",
    ] {
        let refusal = format!("{{\"check\":\"{check}\"}}");
        ok(
            &scratch.root,
            "add_launch",
            &launch(NAME, &[("refusal", &refusal)]),
        );
        assert_eq!(
            stored(&scratch.root, NAME),
            format!("NULL|NULL|NULL|'{check}'|'{check}'|NULL|NULL|NULL|NULL"),
            "{check}: the document does not qualify, so nothing but the refusal is kept"
        );
    }
    ok(
        &scratch.root,
        "add_launch",
        &launch(NAME, &[("refusal", r#"{"check":"resource_names"}"#)]),
    );
    assert_eq!(
        stored(&scratch.root, NAME),
        "'waiting_for_resources'|1|0|'field_shape'|'resource_names'|NULL|NULL|NULL|NULL",
        "the receipt still qualifies as a document, with a forced resources object"
    );
}

#[test]
fn empty_objects_follow_the_measured_reference_behaviour() {
    let _serial = serial();
    let scratch = empty();
    ok(
        &scratch.root,
        "add_launch",
        &launch(NAME, &[("resources", "{}")]),
    );
    assert_eq!(
        stored(&scratch.root, NAME),
        "NULL|NULL|NULL|'field_shape'|'field_shape'|NULL|NULL|NULL|NULL"
    );
    ok(
        &scratch.root,
        "add_launch",
        &launch(NAME, &[("resources", "null"), ("worktree", "{}")]),
    );
    assert_eq!(
        stored(&scratch.root, NAME),
        "NULL|NULL|NULL|'field_shape'|'field_shape'|NULL|NULL|NULL|NULL"
    );
    ok(
        &scratch.root,
        "add_launch",
        &launch(NAME, &[("resources", r#"{"names":[]}"#)]),
    );
    assert_eq!(
        stored(&scratch.root, NAME),
        "'waiting_for_resources'|1|0|'field_shape'|'resource_names'|NULL|NULL|NULL|NULL",
        "refused on the page, yet still a pending-kind receipt for ordinary status"
    );
    assert!(matches!(
        lookup(&scratch.root, &[NAME]).lookups[0],
        Lookup::Pending(_)
    ));
    ok(
        &scratch.root,
        "add_launch",
        &launch(
            NAME,
            &[("resources", "null"), ("worktree", r#"{"path":""}"#)],
        ),
    );
    assert_eq!(
        stored(&scratch.root, NAME),
        "'waiting_for_resources'|0|1|NULL|NULL|'t'|'claude'|'cccccccccccccccccccccccccccccccc'|NULL"
    );
    let found = lookup(&scratch.root, &[NAME]).lookups;
    let Lookup::Pending(pending) = &found[0] else {
        panic!("{found:?}")
    };
    assert_eq!(pending.state, "waiting_for_resources");
}

#[test]
fn invalid_resource_names_are_refused_on_the_page_only() {
    let _serial = serial();
    let scratch = empty();
    for names in [
        r#"["Upper"]"#,
        r#"["a","a"]"#,
        r#"[""]"#,
        r#"["-lead"]"#,
        r#"["has space"]"#,
    ] {
        let resources = format!("{{\"names\":{names}}}");
        ok(
            &scratch.root,
            "add_launch",
            &launch(NAME, &[("resources", &resources)]),
        );
        assert_eq!(
            stored(&scratch.root, NAME),
            "'waiting_for_resources'|1|0|'field_shape'|'resource_names'|NULL|NULL|NULL|NULL",
            "{names}"
        );
    }
    ok(
        &scratch.root,
        "add_launch",
        &launch(NAME, &[("resources", r#"{"names":["ok.name_1:x-y"]}"#)]),
    );
    assert_eq!(
        stored(&scratch.root, NAME),
        "'waiting_for_resources'|1|0|NULL|NULL|'t'|'claude'|'cccccccccccccccccccccccccccccccc'|'[\"ok.name_1:x-y\"]'"
    );
}

#[test]
fn a_complete_launch_keeps_its_display_fields_and_lineage() {
    let _serial = serial();
    let scratch = empty();
    let lineage = format!(
        "{{\"spawned_by\":\"s\",\"root_task_id\":\"{A}\",\"parent_task_id\":\"{B}\",\"chain_root_task_id\":\"junk!\"}}"
    );
    ok(
        &scratch.root,
        "add_launch",
        &launch(
            NAME,
            &[
                ("lineage", &lineage),
                ("state", "\"not_started\""),
                ("not_started_reason", "\"why\""),
            ],
        ),
    );
    assert_eq!(
        query(
            &scratch.root,
            &format!(
                "SELECT spawned_by||'|'||root_task_id||'|'||parent_task_id||'|'||chain_root_task_id||'|'||not_started_reason||'|'||repo_path||'|'||created_at FROM launch_attempts WHERE name='{NAME}'"
            )
        ),
        format!("s|{A}|{B}|junk!|why|/r|2026-11-10T00:00:00+00:00")
    );
}

#[test]
fn adding_a_launch_with_an_existing_name_replaces_it_without_changing_the_name_set() {
    let _serial = serial();
    let scratch = empty();
    ok(&scratch.root, "add_launch", &launch(NAME, &[]));
    ok(&scratch.root, "add_launch", &launch(A, &[]));
    ok(
        &scratch.root,
        "add_invalid_launch_name",
        r#"{"name":"bad name"}"#,
    );
    let names = |root: &Path| {
        query(
            root,
            "SELECT group_concat(name,',') FROM (SELECT name FROM launch_attempts ORDER BY name)",
        )
    };
    let before = names(&scratch.root);
    ok(
        &scratch.root,
        "add_launch",
        &launch(NAME, &[("state", "\"granted\""), ("title", "\"new\"")]),
    );
    assert_eq!(names(&scratch.root), before);
    assert_eq!(
        query(
            &scratch.root,
            &format!("SELECT state||'|'||title FROM launch_attempts WHERE name='{NAME}'")
        ),
        "granted|new"
    );
    assert_eq!(
        query(&scratch.root, "SELECT count(*) FROM launch_attempts"),
        "3"
    );
}

#[test]
fn an_invalid_launch_name_row_has_no_task_id_and_no_fields() {
    let _serial = serial();
    let scratch = empty();
    ok(
        &scratch.root,
        "add_invalid_launch_name",
        r#"{"name":"bad name"}"#,
    );
    assert_eq!(
        query(
            &scratch.root,
            "SELECT quote(task_id)||'|'||quote(state)||'|'||quote(generation)||'|'||quote(refusal_code)||'|'||quote(has_resources) FROM launch_attempts WHERE name='bad name'"
        ),
        "NULL|NULL|NULL|NULL|NULL"
    );
    for body in [
        format!("{{\"name\":\"{A}\"}}"),
        r#"{"name":""}"#.into(),
        r#"{"name":"a/b"}"#.into(),
        r#"{"name":"a\u0000b"}"#.into(),
        r#"{}"#.into(),
    ] {
        let out = run(&scratch.root, "add_invalid_launch_name", &body);
        assert_eq!((out.code, out.stdout.as_str()), (Some(1), ""), "{body}");
    }
}
