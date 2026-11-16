#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
//! Public argument contract for the token-confirmed state recovery command.
use hf_interfaces::ctl::cli::{Parsed, help_text, parse_ctl};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn recover(values: &[&str]) -> Parsed {
    parse_ctl(&args(values), true).expect("valid state-recover arguments")
}

#[test]
fn operation_and_confirmation_are_required_and_preserved() {
    let error = parse_ctl(&args(&["state-recover"]), true).unwrap_err();
    assert_eq!(error.program, "harflow-ctl state-recover");
    assert_eq!(
        error.message,
        "the following arguments are required: --operation-id, --confirmation-token"
    );

    let error = parse_ctl(&args(&["state-recover", "--operation-id", "op-1"]), true).unwrap_err();
    assert_eq!(
        error.message,
        "the following arguments are required: --confirmation-token"
    );

    assert_eq!(
        recover(&[
            "state-recover",
            "--operation-id",
            "op-1",
            "--confirmation-token",
            "opaque.token",
        ]),
        Parsed::StateRecover {
            json: false,
            operation_id: "op-1".into(),
            confirmation_token: "opaque.token".into(),
        }
    );
}

#[test]
fn unique_option_prefixes_equals_values_and_json_are_supported() {
    assert_eq!(
        recover(&[
            "state-recover",
            "--oper=op-2",
            "--confirm=token-value",
            "--j",
        ]),
        Parsed::StateRecover {
            json: true,
            operation_id: "op-2".into(),
            confirmation_token: "token-value".into(),
        }
    );
}

#[test]
fn unknown_and_surplus_arguments_are_rejected() {
    let base = [
        "state-recover",
        "--operation-id",
        "op-1",
        "--confirmation-token",
        "token",
    ];
    let mut unknown = args(&base);
    unknown.push("--force".into());
    assert!(
        parse_ctl(&unknown, true)
            .unwrap_err()
            .message
            .contains("unrecognized arguments: --force")
    );

    let mut surplus = args(&base);
    surplus.push("extra".into());
    assert!(
        parse_ctl(&surplus, true)
            .unwrap_err()
            .message
            .contains("unrecognized arguments: extra")
    );
}

#[test]
fn command_is_unavailable_without_store_read_support() {
    let error = parse_ctl(
        &args(&[
            "state-recover",
            "--operation-id",
            "op-1",
            "--confirmation-token",
            "token",
        ]),
        false,
    )
    .unwrap_err();
    assert!(error.message.contains("invalid choice: 'state-recover'"));
}

#[test]
fn command_help_documents_both_required_confirmation_fields() {
    assert_eq!(
        parse_ctl(&args(&["state-recover", "--help"]), true).unwrap(),
        Parsed::Help(Some("state-recover"))
    );
    let usage = help_text(Some("state-recover"));
    assert!(usage.contains("state-recover"));
    assert!(usage.contains("--operation-id OPERATION_ID"));
    assert!(usage.contains("--confirmation-token CONFIRMATION_TOKEN"));
}
