#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Chain stops and stored chain roots through the real process. Stored columns are read with
//! the system `sqlite3`.
mod support;
use std::path::Path;
use support::{A, B, C, D, E, empty, esc, ok, query, run, serial, task};

const F: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";
const G: &str = "99999999-9999-4999-8999-999999999999";

/// An `add_task` request around the spec of `task`.
fn spec(id: &str, over: &[(&str, &str)]) -> String {
    format!("{{\"spec\":{}}}", task(id, over))
}

#[test]
fn stop_chain_stores_each_mode_and_refuses_a_second_marker() {
    let _serial = serial();
    let scratch = empty();
    let stop = |mode: &str, attribution: &str| {
        let body =
            format!("{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"{mode}\"{attribution}}}}}");
        // A second marker or fault for the same root is refused; clear it first.
        let clear = format!("{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"absent\"}}}}");
        ok(&scratch.root, "stop_chain", &clear);
        ok(&scratch.root, "stop_chain", &body);
        let again = run(&scratch.root, "stop_chain", &body);
        assert_eq!((again.code, again.stdout.as_str()), (Some(1), ""), "{body}");
        query(
            &scratch.root,
            &format!(
                "SELECT quote(kind)||'|'||quote(cancelled_task_id) FROM chain_stops WHERE root='{A}'"
            ),
        )
    };
    let attribution = |kind: &str, id: &str| {
        format!(
            ",\"attribution\":{{\"kind\":\"{kind}\",\"cancelled_task_id\":{}}}",
            esc(id)
        )
    };
    assert_eq!(
        stop("marker", &attribution("valid", B)),
        format!("'marker'|'{B}'")
    );
    assert_eq!(
        stop("marker", &attribution("invalid", "not an id!")),
        "'marker'|'not an id!'"
    );
    assert_eq!(
        stop(
            "marker",
            ",\"attribution\":{\"kind\":\"malformed\",\"cancelled_task_id\":null}"
        ),
        "'marker'|NULL"
    );
    let oversize = query(
        &{
            stop("marker", &attribution("oversize", B));
            scratch.root.clone()
        },
        &format!("SELECT length(cancelled_task_id) FROM chain_stops WHERE root='{A}'"),
    );
    assert_eq!(oversize, (B.len() + 4096).to_string());
    assert_eq!(stop("fault", ""), "'fault'|NULL");
    assert_eq!(
        query(&scratch.root, "SELECT count(*) FROM chain_stops"),
        "1"
    );
    ok(
        &scratch.root,
        "stop_chain",
        &format!("{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"absent\"}}}}"),
    );
    assert_eq!(
        query(&scratch.root, "SELECT count(*) FROM chain_stops"),
        "0"
    );
    ok(
        &scratch.root,
        "stop_chain",
        &format!("{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"absent\"}}}}"),
    );
}

#[test]
fn stop_chain_refuses_malformed_requests() {
    let _serial = serial();
    let scratch = empty();
    for body in [
        format!("{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"other\"}}}}"),
        format!("{{\"spec\":{{\"chain_root\":\"{A}\"}}}}"),
        "{\"spec\":{\"chain_root\":\"bad id\",\"mode\":\"absent\"}}".to_owned(),
        format!("{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"marker\"}}}}"),
        format!(
            "{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"marker\",\"attribution\":{{\"kind\":\"valid\",\"cancelled_task_id\":\"bad id\"}}}}}}"
        ),
        format!(
            "{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"marker\",\"attribution\":{{\"kind\":\"valid\",\"cancelled_task_id\":null}}}}}}"
        ),
        format!(
            "{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"marker\",\"attribution\":{{\"kind\":\"other\",\"cancelled_task_id\":\"x\"}}}}}}"
        ),
        format!("{{\"spec\":{{\"chain_root\":\"{A}\",\"mode\":\"absent\",\"extra\":1}}}}"),
    ] {
        let out = run(&scratch.root, "stop_chain", &body);
        assert_eq!((out.code, out.stdout.as_str()), (Some(1), ""), "{body}");
    }
    assert_eq!(
        query(&scratch.root, "SELECT count(*) FROM chain_stops"),
        "0"
    );
}

fn chain_root(root: &Path, id: &str) -> String {
    query(
        root,
        &format!("SELECT quote(chain_root) FROM task_facts WHERE id='{id}'"),
    )
}

fn header_root(root: &Path, id: &str) -> String {
    query(
        root,
        &format!(
            "SELECT quote(json_extract(CAST(header AS TEXT),'$.chain_root_task_id')) FROM task_headers WHERE id='{id}'"
        ),
    )
}

#[test]
fn add_task_defaults_the_chain_root_to_the_predecessors_root_else_the_predecessor() {
    let _serial = serial();
    let scratch = empty();
    let link = |id: &str, of: &str| {
        ok(
            &scratch.root,
            "add_task",
            &spec(id, &[("continuation_of", &esc(of))]),
        );
    };
    ok(&scratch.root, "add_task", &spec(A, &[]));
    link(B, A);
    link(C, B);
    for (id, expected) in [
        (A, "NULL".to_owned()),
        (B, format!("'{A}'")),
        (C, format!("'{A}'")),
    ] {
        assert_eq!(chain_root(&scratch.root, id), expected, "{id}");
        assert_eq!(header_root(&scratch.root, id), expected, "header {id}");
    }
    // A missing predecessor refuses the operation and creates no task.
    let missing = spec(D, &[("continuation_of", &esc(G))]);
    let out = run(&scratch.root, "add_task", &missing);
    assert_eq!((out.code, out.stdout.as_str()), (Some(1), ""));
    assert_eq!(
        query(
            &scratch.root,
            &format!("SELECT count(*) FROM task_facts WHERE id='{D}'")
        ),
        "0"
    );
    // An explicit chain root needs no predecessor (a superset of the reference adapter).
    let explicit = spec(
        D,
        &[("continuation_of", &esc(G)), ("chain_root", &esc("junk!"))],
    );
    ok(&scratch.root, "add_task", &explicit);
    assert_eq!(chain_root(&scratch.root, D), "'junk!'");
    let explicit_none = spec(F, &[("continuation_of", &esc(G)), ("chain_root", "null")]);
    ok(&scratch.root, "add_task", &explicit_none);
    assert_eq!(chain_root(&scratch.root, F), "NULL");
}

#[test]
fn seed_world_resolves_a_predecessor_named_later_in_the_same_world() {
    let _serial = serial();
    let scratch = empty();
    ok(&scratch.root, "remove_root", "{}");
    let world = format!(
        "{{\"world\":{{\"tasks\":[{},{},{}],\"events\":{{}}}}}}",
        task(B, &[("continuation_of", &esc(A))]),
        task(A, &[]),
        task(C, &[("continuation_of", &esc(C))]),
    );
    ok(&scratch.root, "seed_world", &world);
    assert_eq!(chain_root(&scratch.root, B), format!("'{A}'"));
    assert_eq!(chain_root(&scratch.root, C), format!("'{C}'"));
    // A predecessor outside the world still refuses.
    let outside = scratch.root.with_file_name("outside");
    let world = format!(
        "{{\"world\":{{\"tasks\":[{}],\"events\":{{}}}}}}",
        task(B, &[("continuation_of", &esc(G))]),
    );
    let out = run(&outside, "seed_world", &world);
    assert_eq!(out.code, Some(1));
}

#[test]
fn add_task_accepts_an_explicit_null_a_verbatim_string_and_an_empty_string() {
    let _serial = serial();
    let scratch = empty();
    ok(&scratch.root, "add_task", &spec(A, &[]));
    ok(
        &scratch.root,
        "add_task",
        &spec(B, &[("continuation_of", &esc(A)), ("chain_root", "null")]),
    );
    ok(
        &scratch.root,
        "add_task",
        &spec(
            C,
            &[("continuation_of", &esc(A)), ("chain_root", &esc("junk!"))],
        ),
    );
    ok(
        &scratch.root,
        "add_task",
        &spec(D, &[("continuation_of", &esc(A)), ("chain_root", &esc(""))]),
    );
    ok(
        &scratch.root,
        "add_task",
        &spec(E, &[("continuation_of", &esc(D))]),
    );
    ok(
        &scratch.root,
        "add_task",
        &spec(F, &[("chain_root", &esc(&"a".repeat(70)))]),
    );
    assert_eq!(chain_root(&scratch.root, B), "NULL");
    assert_eq!(chain_root(&scratch.root, C), "'junk!'");
    assert_eq!(chain_root(&scratch.root, D), "''");
    assert_eq!(
        chain_root(&scratch.root, E),
        format!("'{D}'"),
        "an empty stored root is not inherited: the predecessor becomes the root"
    );
    assert_eq!(
        chain_root(&scratch.root, F),
        format!("'{}'", "a".repeat(70))
    );
    let out = run(&scratch.root, "add_task", &spec(G, &[("chain_root", "7")]));
    assert_eq!((out.code, out.stdout.as_str()), (Some(1), ""));
}

#[test]
fn a_damaged_predecessor_refuses_the_default_chain_root_and_creates_no_task() {
    let _serial = serial();
    let scratch = empty();
    ok(&scratch.root, "add_task", &spec(A, &[]));
    ok(
        &scratch.root,
        "damage_task",
        &format!("{{\"task_id\":\"{A}\"}}"),
    );
    let out = run(
        &scratch.root,
        "add_task",
        &spec(B, &[("continuation_of", &esc(A))]),
    );
    assert_eq!((out.code, out.stdout.as_str()), (Some(1), ""));
    assert_eq!(
        query(
            &scratch.root,
            &format!("SELECT count(*) FROM task_facts WHERE id='{B}'")
        ),
        "0"
    );
    // An explicit chain root needs no usable predecessor.
    let explicit = spec(
        B,
        &[("continuation_of", &esc(A)), ("chain_root", &esc("junk!"))],
    );
    ok(&scratch.root, "add_task", &explicit);
    assert_eq!(chain_root(&scratch.root, B), "'junk!'");
}
