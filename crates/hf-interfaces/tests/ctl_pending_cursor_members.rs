#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The members of the wire cursor of `task-pending-page`: the exact set of eight, and the
//! type and range of each, as the reference checks them.
mod ctl_reads_support;
use ctl_reads_support::pending_cursor::*;

#[test]
fn the_member_set_must_be_exactly_the_eight_of_the_reference() {
    let fake = world();
    for from in [
        "\"uncertain\":false,",
        "\"v\":1",
        "\"scope\":",
        "\"root\":",
        "\"identity\":",
        "\"revision\":",
        "\"last\":",
        "\"limit\":2,",
    ] {
        let missing = body(|text| text.replacen(from, "", 1));
        assert_ne!(missing, good());
        assert!(!accepted(&fake, &url(&missing)), "without {from}");
    }
    let extra = body(|text| text.replace("\"v\":1", "\"v\":1,\"extra\":0"));
    assert!(!accepted(&fake, &url(&extra)), "an extra member");
    let renamed = body(|text| text.replace("\"uncertain\"", "\"Uncertain\""));
    assert!(!accepted(&fake, &url(&renamed)));
}

#[test]
fn a_duplicate_member_takes_the_last_value_as_python_does() {
    let fake = world();
    let last_valid = body(|text| text.replace("\"v\":1", "\"v\":2,\"v\":1"));
    assert!(accepted(&fake, &url(&last_valid)), "{last_valid}");
    let last_invalid = body(|text| text.replace("\"v\":1", "\"v\":1,\"v\":2"));
    assert!(!accepted(&fake, &url(&last_invalid)), "{last_invalid}");
    let doubled = body(|text| text.replace("\"limit\":2", "\"limit\":2,\"limit\":2"));
    assert!(accepted(&fake, &url(&doubled)));
}

#[test]
fn the_version_and_scope_are_exact() {
    let fake = world();
    for edit in [
        replaced("\"v\":1", "\"v\":2"),
        replaced("\"v\":1", "\"v\":true"),
        replaced("\"v\":1", "\"v\":1.0"),
        replaced("\"v\":1", "\"v\":\"1\""),
        replaced("\"v\":1", "\"v\":null"),
        replaced(SCOPE, "other_scope"),
        replaced(SCOPE, &format!("{SCOPE} ")),
    ] {
        assert!(!accepted(&fake, &edit), "{edit}");
    }
}

#[test]
fn the_uncertain_flag_must_be_a_real_boolean() {
    let fake = world();
    for edit in ["0", "1", "\"false\"", "null", "false.0"] {
        let text =
            body(|text| text.replace("\"uncertain\":false", &format!("\"uncertain\":{edit}")));
        assert!(!accepted(&fake, &url(&text)), "{edit}");
    }
    let text = body(|text| text.replace("\"uncertain\":false", "\"uncertain\":true"));
    assert!(accepted(&fake, &url(&text)));
}

#[test]
fn the_limit_is_a_whole_number_from_1_to_100_and_not_a_boolean() {
    let fake = world();
    for bad in [
        "true",
        "0",
        "101",
        "-1",
        "2.0",
        "2.5",
        "\"2\"",
        "null",
        "1e2",
        "99999999999999999999",
    ] {
        let text = body(|text| text.replace("\"limit\":2", &format!("\"limit\":{bad}")));
        assert!(!accepted(&fake, &url(&text)), "limit {bad}");
    }
    for ok in ["1", "100"] {
        let text = body(|text| text.replace("\"limit\":2", &format!("\"limit\":{ok}")));
        let out = page(&fake, &url(&text), ok);
        assert_ne!(error_check(&out), "cursor_shape", "limit {ok}");
    }
}

#[test]
fn the_identity_is_four_whole_numbers() {
    let fake = world();
    for bad in [
        "[1,2,1]",
        "[1,2,1,3,4]",
        "[1,2,1,true]",
        "[1,2,1,1.0]",
        "[1,2,1,\"3\"]",
        "[1,2,1,null]",
        "\"1,2,1,3\"",
        "{}",
        "null",
    ] {
        let text = body(|text| text.replace("[1,2,1,3]", bad));
        assert!(!accepted(&fake, &url(&text)), "{bad}");
    }
    // A negative zero is the number 0; other numbers are read, then compared with the store.
    let text = body(|text| text.replace("[1,2,1,3]", "[-0,2,1,3]"));
    let out = page(&fake, &url(&text), "2");
    assert_eq!(
        error(&out),
        ("pending_cursor_expired".into(), "namespace_changed".into())
    );
}

#[test]
fn root_and_revision_are_64_lowercase_hex_characters() {
    let fake = world();
    for (member, value) in [("root", "f"), ("revision", "a")] {
        let original = value.repeat(64);
        for bad in [
            value.repeat(63),
            value.repeat(65),
            "F".repeat(64),
            "g".repeat(64),
            format!("{} ", value.repeat(63)),
            String::new(),
        ] {
            let text = body(|text| {
                text.replace(
                    &format!("\"{member}\":\"{original}\""),
                    &format!("\"{member}\":\"{bad}\""),
                )
            });
            assert_ne!(text, good());
            assert!(!accepted(&fake, &url(&text)), "{member} {bad:?}");
        }
        let numeric = body(|text| {
            text.replace(
                &format!("\"{member}\":\"{original}\""),
                &format!("\"{member}\":0"),
            )
        });
        assert!(!accepted(&fake, &url(&numeric)));
    }
}

#[test]
fn the_position_must_be_a_task_identity() {
    let fake = world();
    for bad in ["", "-a", "a b", "a/b", &"a".repeat(65)] {
        let text = body(|text| text.replace(FIRST, bad));
        assert!(!accepted(&fake, &url(&text)), "{bad:?}");
    }
    let number = body(|text| text.replace(&format!("\"{FIRST}\""), "5"));
    assert!(!accepted(&fake, &url(&number)));
}
