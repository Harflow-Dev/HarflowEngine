#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod actor;
mod contention;
mod cuts;
mod fixture;
mod frames;
mod index;
mod lost_reply;
mod release;
mod sequence;
mod store_limits;

use crate::Mutation;
use fixture::{Fixture, apply, deadline, id, observe, prime, send_intent};
use hf_application::conversation::Command;
use hf_domain::conversation::MessageState;

#[test]
fn accepted_source_enrollment_and_queue_state_survive_close_reopen_with_independent_sqlite_oracle()
{
    let _guard = crate::tests::fixture_guard();
    let fixture = Fixture::new("roundtrip");
    let store = fixture.store();
    let enrollment = fixture::seed(&store);
    let initial = store.conversation_enroll(&enrollment, deadline()).unwrap();
    assert_eq!(initial.enrollment.source, enrollment.source);
    let intent = send_intent(&initial, 10, " \n\0 exact UTF-8 \u{3bb}");
    apply(&store, &enrollment, Command::Send(intent));
    let rows = fixture.private_rows();
    assert_eq!(
        rows.len(),
        3,
        "ledger and two immutable indexes are independently visible"
    );
    assert!(fixture.ledger_bytes(&enrollment).starts_with(b"HFCQ1"));
    drop(store);
    let observed = observe(&fixture.reopen(), &enrollment);
    assert_eq!(observed.messages.len(), 1);
    assert_eq!(observed.messages[0].state, MessageState::Queued);
    assert_eq!(
        observed.messages[0].intent.text,
        " \n\0 exact UTF-8 \u{3bb}"
    );
    assert_eq!(
        (
            observed.enrollment.ordinary_turns,
            observed.enrollment.node_attempts
        ),
        (14, 6)
    );
}

#[test]
fn generic_batch_cannot_modify_private_conversation_namespace() {
    let _guard = crate::tests::fixture_guard();
    let fixture = Fixture::new("namespace");
    let enrollment = prime(&fixture, 0, false);
    let store = fixture.reopen();
    let before = fixture.private_rows();
    for mutation in [
        Mutation::Insert {
            key: fixture::ledger_key(&enrollment),
            value: vec![0],
        },
        Mutation::Put {
            key: fixture::source_key(&enrollment),
            value: vec![1],
        },
        Mutation::Delete {
            key: fixture::context_key(&enrollment),
        },
    ] {
        assert!(matches!(
            store.apply_batch(&[mutation], deadline()),
            Err(super::super::Error::InvalidOwnership)
        ));
    }
    assert_eq!(fixture.private_rows(), before);
    assert_eq!(observe(&store, &enrollment).messages.len(), 0);
    assert_eq!(id(7), enrollment.conversation);
}

fn assert_private_keys_refused(store: &crate::Store, fixture: &Fixture, ledger_key: &str) {
    let normal = "normal-key".to_owned();
    let private = |value: u8| Mutation::Put {
        key: ledger_key.to_owned(),
        value: vec![value],
    };
    let plain = || Mutation::Put {
        key: normal.clone(),
        value: vec![9],
    };
    let rows = fixture.private_rows();
    for mutation in [
        Mutation::Insert {
            key: ledger_key.to_owned(),
            value: vec![0],
        },
        private(0),
        Mutation::Delete {
            key: ledger_key.to_owned(),
        },
    ] {
        let result = store.apply_batch(&[mutation], deadline());
        assert!(
            matches!(result, Err(super::super::Error::InvalidOwnership)),
            "{result:?}"
        );
    }
    for batch in [[plain(), private(0)], [private(0), plain()]] {
        let result = store.apply_batch(&batch, deadline());
        assert!(
            matches!(result, Err(super::super::Error::InvalidOwnership)),
            "{result:?}"
        );
        let read = store
            .read_batch(std::slice::from_ref(&normal), deadline())
            .unwrap();
        assert!(read.is_empty(), "the normal key must not land");
    }
    assert_eq!(fixture.private_rows(), rows);
}

#[test]
fn generic_batches_with_private_keys_are_refused_before_and_after_enrollment() {
    let _guard = crate::tests::fixture_guard();
    let fixture = Fixture::new("batch-private");
    let store = fixture.store();
    let enrollment = fixture::seed(&store);
    assert_private_keys_refused(&store, &fixture, &fixture::ledger_key(&enrollment));
    assert!(fixture.private_rows().is_empty());

    let ledger = store.conversation_enroll(&enrollment, deadline()).unwrap();
    let bytes = fixture.ledger_bytes(&enrollment);
    assert_private_keys_refused(&store, &fixture, &fixture::ledger_key(&enrollment));
    assert_private_keys_refused(&store, &fixture, &fixture::source_key(&enrollment));
    assert_private_keys_refused(&store, &fixture, &fixture::context_key(&enrollment));
    assert_eq!(fixture.ledger_bytes(&enrollment), bytes);
    assert_eq!(observe(&store, &enrollment), ledger);
    // Control: a batch of only normal keys is accepted.
    let normal = "normal-key".to_owned();
    store
        .apply_batch(
            &[Mutation::Put {
                key: normal.clone(),
                value: vec![9],
            }],
            deadline(),
        )
        .unwrap();
    assert_eq!(store.read_batch(&[normal], deadline()).unwrap().len(), 1);
}
