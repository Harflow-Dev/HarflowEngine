use super::support::*;

#[test]
fn terminal_refuses_unreconciled_data_and_input() {
    let (mut owner, _, _) = established_coordinator();
    for drain in [
        drain_evidence(false, true, true),
        drain_evidence(true, false, true),
        drain_evidence(true, true, false),
    ] {
        assert_invalid(&mut owner, eof_append(Vec::new(), drain));
    }
    assert_invalid(
        &mut owner,
        eof_append(vec![1], drain_evidence(true, true, true)),
    );
    assert_finish_invalid(&mut owner);
}

#[test]
fn terminal_waits_for_joined_reconciled_input_and_both_pipe_eof() {
    let (mut owner, binding, accepted) = established_coordinator();
    commit_data_append(&mut owner, &binding, &accepted);
    commit_eof_append(&mut owner, &binding, &accepted, 2, true, false);
    assert_finish_invalid(&mut owner);

    commit_eof_append(&mut owner, &binding, &accepted, 3, true, true);
    let terminal = StorageCommand::Finish(ProcessExit::Code(0));
    let capture = capture_at(&binding, 4, 1, 0, true, true, ProcessExit::Code(0));
    commit(
        &mut owner,
        terminal,
        4,
        accepted,
        Some(capture.clone()),
        None,
    );
    assert_eq!(owner.capture(), Some(&capture));
}

fn assert_invalid(owner: &mut StorageCoordinator, command: StorageCommand) {
    assert_eq!(
        owner
            .begin(command, digest('c'), deadline(), Instant::now())
            .unwrap_err(),
        CoordinatorError::InvalidState
    );
}

fn assert_finish_invalid(owner: &mut StorageCoordinator) {
    assert_invalid(owner, StorageCommand::Finish(ProcessExit::Code(0)));
}

fn drain_evidence(
    reader_joined: bool,
    accepted_data_reconciled: bool,
    input_reconciled: bool,
) -> TrustedDrainEvidence {
    TrustedDrainEvidence {
        reader_joined,
        accepted_data_reconciled,
        input_reconciled,
        stdout_eof: true,
        stderr_eof: true,
    }
}

fn eof_append(bytes: Vec<u8>, drain: TrustedDrainEvidence) -> StorageCommand {
    StorageCommand::Append {
        stream: Stream::Stdout,
        bytes,
        drain: Some(drain),
    }
}

fn commit_data_append(
    owner: &mut StorageCoordinator,
    binding: &CaptureBinding,
    accepted: &Snapshot,
) {
    let command = StorageCommand::Append {
        stream: Stream::Stdout,
        bytes: vec![9],
        drain: None,
    };
    let capture = capture_at(binding, 1, 1, 0, false, false, ProcessExit::Unobserved);
    commit(owner, command, 1, accepted.clone(), Some(capture), None);
}

fn commit_eof_append(
    owner: &mut StorageCoordinator,
    binding: &CaptureBinding,
    accepted: &Snapshot,
    revision: u64,
    stdout_eof: bool,
    stderr_eof: bool,
) {
    let command = StorageCommand::Append {
        stream: Stream::Stderr,
        bytes: Vec::new(),
        drain: Some(TrustedDrainEvidence {
            reader_joined: true,
            accepted_data_reconciled: true,
            input_reconciled: true,
            stdout_eof,
            stderr_eof,
        }),
    };
    let capture = capture_at(
        binding,
        revision,
        1,
        0,
        stdout_eof,
        stderr_eof,
        ProcessExit::Unobserved,
    );
    commit(
        owner,
        command,
        revision,
        accepted.clone(),
        Some(capture),
        None,
    );
}
