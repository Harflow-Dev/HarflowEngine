#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Launch attempts and chain stops: the CHECK constraints of each row class.
mod common;
mod fixture_support;
use fixture_support::{A, B, World, quoted, total, world};
use rusqlite::ErrorCode;

const COLUMNS: &str = "name,task_id,generation,state,title,backend,repo_path,created_at,not_started_reason,spawned_by,root_task_id,parent_task_id,chain_root_task_id,resource_names,has_resources,has_worktree,refusal_code,refusal_check";
const ROW: &str = "launch_attempts";
const GEN: &str = "'0123456789abcdef0123456789abcdef'";

fn insert(w: &World, sql_values: &str) -> rusqlite::Result<usize> {
    w.raw().execute(
        &format!("INSERT INTO {ROW}({COLUMNS}) VALUES({sql_values})"),
        [],
    )
}

fn refused(w: &World, sql_values: &str) {
    let error = insert(w, sql_values).unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation),
        "{sql_values}: {error}"
    );
}

/// A complete launch named `name`: only the listed columns differ in the rows below.
fn complete(name: &str, state: &str) -> String {
    format!(
        "'{name}','{name}',{GEN},'{state}','t','claude','/r','c',NULL,NULL,NULL,NULL,NULL,NULL,0,0,NULL,NULL"
    )
}

#[test]
fn each_row_class_is_accepted() {
    let w = world();
    insert(&w, &complete(A, "reserved")).unwrap();
    insert(
        &w,
        &format!("'{B}','{B}',NULL,'granted',NULL,NULL,NULL,NULL,'r',NULL,NULL,NULL,NULL,NULL,1,0,'field_shape','resource_names'"),
    )
    .unwrap();
    insert(
        &w,
        "'not an id',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL",
    )
    .unwrap();
    assert_eq!(total(&w.db(), ROW), 3);
    // A refusal with no readable status columns (the receipt is not even a launch document).
    insert(
        &w,
        "'cc','cc',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'format','format'",
    )
    .unwrap();
}

#[test]
fn the_state_must_be_one_of_the_seven_values() {
    let w = world();
    for (n, state) in [
        "reserved",
        "claimed",
        "waiting_for_resources",
        "granted",
        "spawned",
        "settled",
        "not_started",
    ]
    .into_iter()
    .enumerate()
    {
        insert(&w, &complete(&format!("ok{n}"), state)).unwrap();
    }
    refused(&w, &complete("bad", "running"));
    refused(&w, &complete("bad", ""));
    refused(&w, &complete("bad", "Reserved"));
}

#[test]
fn an_invalid_name_row_has_a_null_task_id_and_null_everything_else() {
    let w = world();
    let nulls = |i: usize, value: &str| {
        let mut cells = vec!["NULL"; 17];
        cells[i] = value;
        format!("'weird name',{}", cells.join(","))
    };
    for (index, value) in [
        (1usize, "'x'"),
        (2, GEN),
        (3, "'reserved'"),
        (4, "'t'"),
        (5, "'claude'"),
        (6, "'/r'"),
        (7, "'c'"),
        (8, "'r'"),
        (9, "'s'"),
        (10, "'r'"),
        (11, "'p'"),
        (12, "'c'"),
        (13, "'[]'"),
        (14, "1"),
        (15, "0"),
        (16, "'x'"),
    ] {
        refused(&w, &nulls(index, value));
    }
    refused(
        &w,
        "'',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL",
    );
}

#[test]
fn a_task_id_must_equal_the_name() {
    let w = world();
    refused(
        &w,
        &complete(A, "reserved").replacen(&format!("'{A}','{A}'"), &format!("'{A}','{B}'"), 1),
    );
}

#[test]
fn a_refused_row_has_both_refusal_columns_and_no_display_fields() {
    let w = world();
    let row = |edit: &dyn Fn(&mut Vec<String>)| {
        let mut cells: Vec<String> = format!(
            "'{A}','{A}',NULL,'reserved',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,0,0,'field_shape','field_shape'"
        )
        .split(',')
        .map(str::to_owned)
        .collect();
        edit(&mut cells);
        cells.join(",")
    };
    insert(&w, &row(&|_| {})).unwrap();
    let w = world();
    refused(&w, &row(&|c| c[16] = "NULL".into()));
    refused(&w, &row(&|c| c[17] = "NULL".into()));
    for (index, value) in [
        (2usize, GEN),
        (4, "'t'"),
        (5, "'claude'"),
        (6, "'/r'"),
        (7, "'c'"),
        (9, "'s'"),
        (10, "'r'"),
        (11, "'p'"),
        (12, "'c'"),
    ] {
        refused(&w, &row(&|c| c[index] = value.into()));
    }
    refused(
        &w,
        &row(&|c| {
            c[14] = "1".into();
            c[13] = "'[\"a\"]'".into();
        }),
    );
    refused(&w, &row(&|c| c[1] = "NULL".into()));
}

#[test]
fn an_unrefused_launch_needs_a_generation_a_state_and_a_backend() {
    let w = world();
    let without = |index: usize| {
        let mut cells: Vec<String> = complete(A, "reserved")
            .split(',')
            .map(str::to_owned)
            .collect();
        cells[index] = "NULL".into();
        cells.join(",")
    };
    refused(&w, &without(2));
    refused(&w, &without(5));
    // State, has_resources and has_worktree are present or absent together.
    refused(&w, &without(3));
    refused(&w, &without(14));
    refused(&w, &without(15));
}

#[test]
fn flags_are_booleans_and_resource_names_need_the_resources_flag() {
    let w = world();
    let mut cells: Vec<String> = complete(A, "reserved")
        .split(',')
        .map(str::to_owned)
        .collect();
    cells[14] = "2".into();
    refused(&w, &cells.join(","));
    cells[14] = "0".into();
    cells[13] = "'[\"a\"]'".into();
    refused(&w, &cells.join(","));
    cells[14] = "1".into();
    insert(&w, &cells.join(",")).unwrap();
}

#[test]
fn the_name_is_unique_and_non_empty() {
    let w = world();
    insert(&w, &complete(A, "reserved")).unwrap();
    assert!(insert(&w, &complete(A, "claimed")).is_err());
    assert_eq!(
        quoted(&w.db(), ROW, "state", "name", A),
        "'reserved'",
        "the failed insert changed nothing"
    );
}

#[test]
fn chain_stop_rows_enforce_kind_attribution_and_root() {
    let w = world();
    let ok = |sql: &str| w.raw().execute_batch(sql).unwrap();
    ok("INSERT INTO chain_stops VALUES('r1','marker','whatever text')");
    ok("INSERT INTO chain_stops VALUES('r2','marker',NULL)");
    ok("INSERT INTO chain_stops VALUES('r3','fault',NULL)");
    ok(&format!(
        "INSERT INTO chain_stops VALUES('{}','marker','')",
        "a".repeat(64)
    ));
    let long = "a".repeat(65);
    for bad in [
        "INSERT INTO chain_stops VALUES('r4','other',NULL)".to_owned(),
        "INSERT INTO chain_stops VALUES('r5','fault','x')".to_owned(),
        "INSERT INTO chain_stops VALUES('','marker',NULL)".to_owned(),
        format!("INSERT INTO chain_stops VALUES('{long}','marker',NULL)"),
        "INSERT INTO chain_stops VALUES('r7',NULL,NULL)".to_owned(),
        "INSERT INTO chain_stops VALUES('r8','marker',x'00')".to_owned(),
        "INSERT INTO chain_stops VALUES('r1','marker',NULL)".to_owned(),
    ] {
        let error = w.raw().execute_batch(&bad).unwrap_err();
        assert_eq!(
            error.sqlite_error_code(),
            Some(ErrorCode::ConstraintViolation),
            "{bad}: {error}"
        );
    }
}

#[test]
fn a_task_id_must_have_the_task_id_shape() {
    let w = world();
    for bad in ["bad id!", "-lead", "_lead", &"a".repeat(65), "tab\tid"] {
        refused(&w, &complete(bad, "reserved"));
    }
    insert(&w, &complete("Ok_id-9", "reserved")).unwrap();
    insert(&w, &complete(&"a".repeat(64), "reserved")).unwrap();
}

#[test]
fn resource_names_must_be_null_or_a_json_array() {
    let w = world();
    let with_names = |text: &str| {
        format!(
            "'{A}','{A}',{GEN},'reserved','t','claude','/r','c',NULL,NULL,NULL,NULL,NULL,{text},1,0,NULL,NULL"
        )
    };
    for bad in ["'not json'", "'{\"a\":1}'", "'\"x\"'", "'[1'", "7"] {
        refused(&w, &with_names(bad));
    }
    insert(&w, &with_names("'[\"a\",\"b\"]'")).unwrap();
}

#[test]
fn a_task_id_or_name_with_an_embedded_nul_is_refused() {
    let w = world();
    for name in ["a\u{0}!", "a\u{0}", "a\u{0}b"] {
        let error = w
            .raw()
            .execute(
                &format!("INSERT INTO {ROW}({COLUMNS}) VALUES(?1,?1,{GEN},'reserved','t','claude','/r','c',NULL,NULL,NULL,NULL,NULL,NULL,0,0,NULL,NULL)"),
                [name],
            )
            .unwrap_err();
        assert_eq!(
            error.sqlite_error_code(),
            Some(ErrorCode::ConstraintViolation),
            "{name:?}: {error}"
        );
    }
    assert_eq!(total(&w.db(), ROW), 0);
}
