//! Git preparation rules of nodes: `validate_request` and `validate_node`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use hf_domain::workflow::git::{GitRequest, validate_node, validate_request};
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::{Refusal, Rule};
use hf_domain::workflow::rule_shape::GitRule;

fn request<'a>() -> GitRequest<'a> {
    GitRequest {
        freedom: "write_in_repo",
        branch: None,
        base: None,
        fetch: None,
        fetch_required: None,
        worktree: false,
        continuation: false,
    }
}

fn rule_of(refusal: &Refusal) -> Rule {
    refusal.rule
}

fn text(value: &str) -> Json {
    Json::text(value)
}

fn names(list: &[&str]) -> Json {
    Json::Array(list.iter().map(|name| text(name)).collect())
}

#[test]
fn validate_request_rules() {
    let (branch, base) = (text("feature/x"), text("origin/develop"));
    let (fetch, required) = (names(&["origin", "upstream"]), Json::Bool(true));
    let full = GitRequest {
        freedom: "publish",
        branch: Some(&branch),
        base: Some(&base),
        fetch: Some(&fetch),
        fetch_required: Some(&required),
        worktree: false,
        continuation: false,
    };
    let plan = validate_request(&full).unwrap();
    assert_eq!(plan.branch.as_deref(), Some("feature/x"));
    assert_eq!(plan.base.as_deref(), Some("origin/develop"));
    assert_eq!(
        plan.fetch,
        Some(vec!["origin".to_string(), "upstream".to_string()])
    );
    assert!(plan.fetch_required);
    // Each rule refuses on its own; the first that applies wins.
    let refusal = validate_request(&GitRequest {
        fetch: Some(&Json::Bool(false)),
        ..full
    })
    .unwrap_err();
    assert_eq!(rule_of(&refusal), GitRule::FetchRequiredNeedsFetch.into());
    let refusal = validate_request(&GitRequest {
        fetch_required: Some(&Json::Null),
        ..full
    })
    .unwrap_err();
    assert_eq!(rule_of(&refusal), GitRule::FetchRequiredNotBool.into());
    let refusal = validate_request(&GitRequest { base: None, ..full }).unwrap_err();
    assert_eq!(rule_of(&refusal), GitRule::BranchBaseTogether.into());
}

#[test]
fn an_empty_request_is_a_plan_without_work() {
    let plan = validate_request(&request()).unwrap();
    assert_eq!(
        (plan.branch, plan.base, plan.fetch, plan.fetch_required),
        (None, None, None, false)
    );
}

#[test]
fn literal_names() {
    for good in ["feature/x", "a_b-c", "a/b/c", "1", "A"] {
        let (branch, base) = (text(good), text("origin/main"));
        let plan = validate_request(&GitRequest {
            branch: Some(&branch),
            base: Some(&base),
            ..request()
        })
        .unwrap();
        assert_eq!(plan.branch.as_deref(), Some(good));
    }
    for bad in [
        "", "-x", "_x", "/x", "x/", "a//b", "a/../b", "a/./b", "..", "a b", "x:y", "x~1", "a.b",
        "x^", "é", "a\n", "--option",
    ] {
        let (branch, base) = (text(bad), text("origin/main"));
        let refusal = validate_request(&GitRequest {
            branch: Some(&branch),
            base: Some(&base),
            ..request()
        })
        .unwrap_err();
        assert_eq!(rule_of(&refusal), GitRule::LiteralName.into(), "{bad:?}");
        assert_eq!(refusal.detail, [Json::Null, text("branch")]);
    }
    let number = Json::int(5);
    let base = text("origin/main");
    let refusal = validate_request(&GitRequest {
        branch: Some(&number),
        base: Some(&base),
        ..request()
    })
    .unwrap_err();
    assert_eq!(rule_of(&refusal), GitRule::LiteralName.into());
}

#[test]
fn base_form() {
    let branch = text("x");
    let check = |base: &Json| {
        validate_request(&GitRequest {
            branch: Some(&branch),
            base: Some(base),
            ..request()
        })
    };
    assert_eq!(
        check(&text("origin/feature/x")).unwrap().base.as_deref(),
        Some("origin/feature/x")
    );
    assert_eq!(
        rule_of(&check(&text("origin")).unwrap_err()),
        GitRule::BaseForm.into()
    );
    for bad in ["origin/", "/main", "or igin/main", "origin//x", "origin/.."] {
        let refusal = check(&text(bad)).unwrap_err();
        assert_eq!(rule_of(&refusal), GitRule::LiteralName.into(), "{bad}");
    }
    let refusal = check(&text("origin/a b")).unwrap_err();
    assert_eq!(refusal.detail, [Json::Null, text("base")]);
}

#[test]
fn branch_and_base_come_together() {
    let value = text("x");
    let only_branch = validate_request(&GitRequest {
        branch: Some(&value),
        ..request()
    })
    .unwrap_err();
    assert_eq!(rule_of(&only_branch), GitRule::BranchBaseTogether.into());
    let only_base = validate_request(&GitRequest {
        base: Some(&value),
        ..request()
    })
    .unwrap_err();
    assert_eq!(rule_of(&only_base), GitRule::BranchBaseTogether.into());
}

#[test]
fn branch_conflicts_in_order() {
    let (branch, base) = (text("x"), text("origin/main"));
    let with = |freedom, worktree, continuation| {
        validate_request(&GitRequest {
            freedom,
            branch: Some(&branch),
            base: Some(&base),
            worktree,
            continuation,
            ..request()
        })
    };
    assert!(with("publish", false, false).is_ok());
    assert_eq!(
        rule_of(&with("read_only", false, false).unwrap_err()),
        GitRule::BranchReadOnly.into()
    );
    assert_eq!(
        rule_of(&with("publish", true, false).unwrap_err()),
        GitRule::BranchWorktree.into()
    );
    assert_eq!(
        rule_of(&with("publish", false, true).unwrap_err()),
        GitRule::BranchContinuation.into()
    );
    assert_eq!(
        rule_of(&with("read_only", true, true).unwrap_err()),
        GitRule::BranchContinuation.into()
    );
    assert_eq!(
        rule_of(&with("read_only", true, false).unwrap_err()),
        GitRule::BranchReadOnly.into()
    );
}

#[test]
fn fetch_forms() {
    let fetch = |value: Option<&Json>, required: Option<&Json>| {
        validate_request(&GitRequest {
            fetch: value,
            fetch_required: required,
            ..request()
        })
    };
    assert_eq!(fetch(None, None).unwrap().fetch, None);
    assert_eq!(fetch(Some(&Json::Bool(false)), None).unwrap().fetch, None);
    assert_eq!(
        fetch(Some(&Json::Bool(true)), None).unwrap().fetch,
        Some(vec!["<default>".to_string()])
    );
    assert_eq!(
        fetch(Some(&names(&["origin", "up"])), None).unwrap().fetch,
        Some(vec!["origin".to_string(), "up".to_string()])
    );
    assert_eq!(
        rule_of(&fetch(Some(&names(&[])), None).unwrap_err()),
        GitRule::FetchEmpty.into()
    );
    assert_eq!(
        rule_of(&fetch(Some(&text("origin")), None).unwrap_err()),
        GitRule::FetchForm.into()
    );
    assert_eq!(
        rule_of(&fetch(Some(&Json::int(1)), None).unwrap_err()),
        GitRule::FetchForm.into()
    );
    let refusal = fetch(Some(&names(&["origin", "bad name"])), None).unwrap_err();
    assert_eq!(refusal.detail, [Json::Null, text("fetch remote")]);
}

#[test]
fn fetch_required_rules() {
    let yes = Json::Bool(true);
    let fetch = |value: Option<&Json>, required: Option<&Json>| {
        validate_request(&GitRequest {
            fetch: value,
            fetch_required: required,
            ..request()
        })
    };
    assert!(fetch(Some(&yes), Some(&yes)).unwrap().fetch_required);
    assert_eq!(
        rule_of(&fetch(None, Some(&yes)).unwrap_err()),
        GitRule::FetchRequiredNeedsFetch.into()
    );
    assert!(
        !fetch(None, Some(&Json::Bool(false)))
            .unwrap()
            .fetch_required
    );
    assert_eq!(
        rule_of(&fetch(None, Some(&Json::int(1))).unwrap_err()),
        GitRule::FetchRequiredNotBool.into()
    );
    assert_eq!(
        rule_of(&fetch(None, Some(&Json::Null)).unwrap_err()),
        GitRule::FetchRequiredNotBool.into()
    );
    let bad_fetch = text("x");
    assert_eq!(
        rule_of(&fetch(Some(&bad_fetch), Some(&Json::int(1))).unwrap_err()),
        GitRule::FetchRequiredNotBool.into()
    );
}

fn node(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect(),
    )
}

#[test]
fn validate_node_prefix_and_default_freedom() {
    let bad = node(&[("branch", text("x"))]);
    let refusal = validate_node(&bad, "n1").unwrap_err();
    assert_eq!(refusal.detail[0], text("n1"));
    let literal = node(&[("branch", text("-x")), ("base", text("origin/main"))]);
    assert_eq!(
        validate_node(&literal, "n2").unwrap_err().detail,
        [text("n2"), text("branch")]
    );
    // Without a freedom the request is judged at write_in_repo: a branch is allowed.
    let open = node(&[("branch", text("x")), ("base", text("origin/main"))]);
    assert!(validate_node(&open, "n").is_ok());
    let read_only = node(&[
        ("branch", text("x")),
        ("base", text("origin/main")),
        ("freedom", text("read_only")),
    ]);
    assert_eq!(
        rule_of(&validate_node(&read_only, "n").unwrap_err()),
        GitRule::BranchReadOnly.into()
    );
    let odd_freedom = node(&[
        ("branch", text("x")),
        ("base", text("origin/main")),
        ("freedom", Json::int(3)),
    ]);
    assert!(validate_node(&odd_freedom, "n").is_ok());
}

#[test]
fn validate_node_treats_null_as_absent() {
    let nulls = node(&[
        ("branch", Json::Null),
        ("base", Json::Null),
        ("fetch", Json::Null),
    ]);
    let plan = validate_node(&nulls, "n").unwrap();
    assert_eq!((plan.branch, plan.base, plan.fetch), (None, None, None));
    let half = node(&[("branch", text("x")), ("base", Json::Null)]);
    assert_eq!(
        rule_of(&validate_node(&half, "n").unwrap_err()),
        GitRule::BranchBaseTogether.into()
    );
    let null_required = node(&[("fetch_required", Json::Null)]);
    assert_eq!(
        rule_of(&validate_node(&null_required, "n").unwrap_err()),
        GitRule::FetchRequiredNotBool.into()
    );
}
