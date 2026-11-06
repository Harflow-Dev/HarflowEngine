#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_domain::process::ProcessIdentity;

#[path = "claude_two_tunnel/client_reason_metadata.rs"]
mod client_reason_metadata;

const NONCE: &str = "0123456789abcdef0123456789abcdef";

fn identity(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 1, 0, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}

fn row(tag: &str, pid: u32, fields: &str) -> String {
    format!("{tag}\t{NONCE}\t{pid}{fields}\n")
}

fn prefix(pid: u32) -> String {
    row("CR2", pid, "")
}

fn terminal(tag: &str, call: usize) -> String {
    format!("{tag}\t{call}\n")
}

fn end(pid: u32, calls: usize, errors: usize, overflow: usize, healthy: usize) -> String {
    row(
        "CX2",
        pid,
        &format!("\t{calls}\t{errors}\t{overflow}\t{healthy}"),
    )
}

fn entry(call: usize) -> String {
    terminal("CI2", call)
}
fn returned(call: usize) -> String {
    terminal("CN2", call)
}
fn threw(call: usize) -> String {
    terminal("CS2", call)
}
fn resolved(call: usize, tag: &str) -> String {
    terminal(tag, call)
}

fn error(pid: u32, call: usize) -> String {
    row("CE2", pid, &format!("\t{call}\tverify\tissuer"))
}

fn parse(
    text: &str,
    nonce: &str,
    pid: u32,
) -> Result<client_reason_metadata::Observation, &'static str> {
    client_reason_metadata::parse(text.as_bytes(), nonce, &identity(pid))
}

#[test]
fn accepts_partial_positive_phases_and_complete_unresolved_calls() {
    let pid = 42;
    let entered = format!("{}{}", prefix(pid), entry(1));
    let observation = parse(&entered, NONCE, pid).unwrap();
    assert_eq!(observation.calls.len(), 1);
    assert_eq!(observation.calls[0].call, 1);
    assert_eq!(observation.calls[0].origin, None);
    assert_eq!(observation.calls[0].terminal, None);
    assert_eq!(observation.reported_calls, None);
    assert!(!observation.complete);

    let returned_prefix = format!("{}{}{}", prefix(pid), entry(1), returned(1));
    let observation = parse(&returned_prefix, NONCE, pid).unwrap();
    assert_eq!(
        observation.calls[0].origin,
        Some(client_reason_metadata::Origin::Returned)
    );
    assert_eq!(observation.calls[0].terminal, None);
    assert_eq!(observation.reported_calls, None);

    let complete = format!(
        "{}{}{}{}",
        prefix(pid),
        entry(1),
        returned(1),
        end(pid, 1, 0, 0, 1)
    );
    let observation = parse(&complete, NONCE, pid).unwrap();
    assert!(observation.complete && observation.healthy);
    assert_eq!(observation.reported_calls, Some(1));
    assert_eq!(observation.calls[0].terminal, None);
    assert!(!observation.overflow);
}

#[test]
fn accepts_each_origin_and_reentrant_interleaving() {
    let pid = 42;
    let rows = format!(
        "{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
        prefix(pid),
        entry(1),
        entry(2),
        threw(2),
        error(pid, 2),
        entry(3),
        returned(3),
        resolved(3, "CF2"),
        entry(4),
        returned(4),
        resolved(4, "CA2"),
        end(pid, 4, 1, 0, 0),
        "",
        ""
    );
    let observation = parse(&rows, NONCE, pid).unwrap();
    assert_eq!(observation.calls.len(), 4);
    assert_eq!(observation.records.len(), 1);
    assert_eq!(observation.records[0].call, 2);
    assert_eq!(
        observation.calls[1].origin,
        Some(client_reason_metadata::Origin::Threw)
    );
    assert_eq!(
        observation.calls[1].terminal,
        Some(client_reason_metadata::Terminal::Error)
    );
    assert_eq!(
        observation.calls[2].terminal,
        Some(client_reason_metadata::Terminal::Fulfilled)
    );
    assert_eq!(
        observation.calls[3].terminal,
        Some(client_reason_metadata::Terminal::AttachmentFault)
    );
}

#[test]
fn accepts_four_async_errors_out_of_order() {
    let pid = 42;
    let four_async_errors = format!(
        "{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
        prefix(pid),
        entry(1),
        returned(1),
        entry(2),
        returned(2),
        entry(3),
        returned(3),
        entry(4),
        returned(4),
        error(pid, 4),
        error(pid, 2),
        error(pid, 1),
        error(pid, 3),
        end(pid, 4, 4, 0, 1),
        ""
    );
    let observation = parse(&four_async_errors, NONCE, pid).unwrap();
    assert_eq!(
        observation
            .records
            .iter()
            .map(|record| record.call)
            .collect::<Vec<_>>(),
        [4, 2, 1, 3]
    );

    let async_order = format!(
        "{}{}{}{}{}{}{}{}{}{}{}{}",
        prefix(pid),
        entry(1),
        returned(1),
        entry(2),
        returned(2),
        error(pid, 2),
        entry(3),
        returned(3),
        resolved(3, "CU2"),
        error(pid, 1),
        end(pid, 3, 2, 0, 0),
        ""
    );
    let observation = parse(&async_order, NONCE, pid).unwrap();
    assert_eq!(
        observation.calls[0].terminal,
        Some(client_reason_metadata::Terminal::Error)
    );
    assert_eq!(
        observation.calls[1].terminal,
        Some(client_reason_metadata::Terminal::Error)
    );
    assert_eq!(
        observation.calls[2].terminal,
        Some(client_reason_metadata::Terminal::ObserverFault)
    );
}

#[test]
fn accepts_one_overflow_marker_and_short_fault_rows_only_with_unhealthy_end() {
    let pid = 42;
    let overflow = format!(
        "{}{}{}{}{}{}{}{}{}{}",
        prefix(pid),
        entry(1),
        returned(1),
        entry(2),
        returned(2),
        entry(3),
        returned(3),
        entry(4),
        returned(4),
        row("CO2", pid, "") + &end(pid, 4, 0, 1, 1)
    );
    let observation = parse(&overflow, NONCE, pid).unwrap();
    assert!(observation.overflow && observation.complete && observation.healthy);
    assert_eq!(observation.reported_calls, Some(4));

    for (tag, origin, terminal_kind) in [
        (
            "CA2",
            "CN2",
            client_reason_metadata::Terminal::AttachmentFault,
        ),
        (
            "CU2",
            "CS2",
            client_reason_metadata::Terminal::ObserverFault,
        ),
    ] {
        let rows = format!(
            "{}{}{}{}{}",
            prefix(pid),
            entry(1),
            terminal(origin, 1),
            terminal(tag, 1),
            end(pid, 1, 0, 0, 0)
        );
        let observation = parse(&rows, NONCE, pid).unwrap();
        assert_eq!(observation.calls[0].terminal, Some(terminal_kind));
    }
    for tag in ["CA2", "CU2"] {
        let rows = format!(
            "{}{}{}{}{}",
            prefix(pid),
            entry(1),
            returned(1),
            resolved(1, tag),
            end(pid, 1, 0, 0, 1)
        );
        assert_eq!(parse(&rows, NONCE, pid), Err("unavailable"));
    }
}

#[test]
fn enforces_exact_512_byte_worst_case_and_rejects_one_byte_over_limit() {
    let pid = 2_147_483_647;
    let mut stream = prefix(pid);
    for call in 1..=4 {
        stream.push_str(&entry(call));
        stream.push_str(&returned(call));
        stream.push_str(&row(
            "CE2",
            pid,
            &format!("\t{call}\tnot_yet_valid\tnot_yet_valid"),
        ));
    }
    stream.push_str(&row("CO2", pid, ""));
    stream.push_str(&end(pid, 4, 4, 1, 1));
    assert_eq!(stream.len(), 512);
    assert!(parse(&stream, NONCE, pid).is_ok());
    stream.push('x');
    assert_eq!(stream.len(), 513);
    assert_eq!(parse(&stream, NONCE, pid), Err("unavailable"));
}

#[test]
fn rejects_malformed_binding_order_duplicates_conflicts_and_inconsistent_counts() {
    let pid = 42;
    let valid = format!(
        "{}{}{}{}{}",
        prefix(pid),
        entry(1),
        returned(1),
        error(pid, 1),
        end(pid, 1, 1, 0, 0)
    );
    let invalid = [
        format!("{}{}", prefix(pid), prefix(pid)),
        format!("{}{}", entry(1), prefix(pid)),
        format!("{}{}", prefix(pid), error(pid, 1)),
        format!(
            "{}{}{}{}",
            prefix(pid),
            entry(1),
            entry(1),
            end(pid, 1, 0, 0, 1)
        ),
        format!(
            "{}{}{}{}{}",
            prefix(pid),
            entry(1),
            returned(1),
            threw(1),
            end(pid, 1, 0, 0, 1)
        ),
        format!(
            "{}{}{}{}{}",
            prefix(pid),
            entry(1),
            threw(1),
            resolved(1, "CF2"),
            end(pid, 1, 0, 0, 1)
        ),
        format!(
            "{}{}{}{}",
            prefix(pid),
            entry(2),
            returned(2),
            end(pid, 1, 0, 0, 1)
        ),
        format!(
            "{}{}{}{}{}",
            prefix(pid),
            entry(1),
            returned(1),
            resolved(1, "CA2"),
            end(pid, 1, 0, 0, 1)
        ),
        valid.replace("\t42\t", "\t43\t"),
        valid.replace(NONCE, "1123456789abcdef0123456789abcdef"),
        valid.replace("CR2", "CR1"),
        valid.replace("verify", "private-text"),
        valid.replace("\n", "\r\n"),
        valid.replace("\t1\tverify", "\t+1\tverify"),
        valid.replace("\t1\tverify", "\t01\tverify"),
        format!("{}{}", prefix(pid), row("NOISE", pid, "")),
        format!(
            "{}{}{}{}{}{}",
            prefix(pid),
            entry(1),
            returned(1),
            error(pid, 1),
            error(pid, 1),
            end(pid, 1, 1, 0, 0)
        ),
    ];
    for bytes in invalid {
        let error = parse(&bytes, NONCE, pid).unwrap_err();
        assert_eq!(error, "unavailable");
        assert!(!error.contains("private-text") && !error.contains(NONCE));
    }
    assert_eq!(parse(&valid, "short", pid), Err("unavailable"));
    assert_eq!(parse(&valid, NONCE, pid + 1), Err("unavailable"));
}

#[test]
fn rejects_inconsistent_counts_and_healthy_fault_end() {
    let pid = 42;
    let valid = format!(
        "{}{}{}{}{}",
        prefix(pid),
        entry(1),
        returned(1),
        error(pid, 1),
        end(pid, 1, 1, 0, 0)
    );
    let invalid = [
        valid.replace("\t1\t1\t0\t0\n", "\t2\t1\t0\t0\n"),
        valid.replace("\t1\t1\t0\t0\n", "\t1\t0\t0\t0\n"),
        valid.replace("\t1\t1\t0\t0\n", "\t1\t1\t1\t0\n"),
        format!(
            "{}{}{}{}{}",
            prefix(pid),
            entry(1),
            returned(1),
            resolved(1, "CU2"),
            end(pid, 1, 0, 0, 1)
        ),
    ];
    for text in invalid {
        assert_eq!(parse(&text, NONCE, pid), Err("unavailable"));
    }
}

#[test]
fn observer_sources_do_not_embed_cli_classifier_needles() {
    const SOURCES: &[&[u8]] = &[
        include_bytes!("claude_two_tunnel/client_reason_classifier.cjs"),
        include_bytes!("claude_two_tunnel/client_reason_hook.cjs"),
        include_bytes!("claude_two_tunnel/client_reason_writer.cjs"),
        include_bytes!("claude_two_tunnel/client_reason_preload.cjs"),
    ];
    let needles = [
        "CERTIFICATE_VERIFY_FAILED",
        "self-signed certificate",
        "unable to verify",
    ];
    for source in SOURCES {
        for needle in needles {
            assert!(
                !source
                    .windows(needle.len())
                    .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
            );
        }
    }
}
