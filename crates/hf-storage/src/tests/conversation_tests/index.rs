//! Index conflicts and missing halves, edited with a raw write connection.
use super::fixture::{Fixture, context_key, deadline, id, ledger_key, observe, prime, source_key};
use crate::{Error, tests::fixture_guard};
use hf_domain::conversation::{Enrollment, Error as DomainError};

fn variant(e: &Enrollment, conversation: u32, operation: u32, context: u32) -> Enrollment {
    let mut value = e.clone();
    value.conversation = id(conversation);
    value.source.operation = id(operation);
    value.fence.operation = id(operation);
    value.context = id(context);
    value
}

#[test]
fn each_missing_half_is_incompatible_and_nothing_else_is_erased() {
    let _guard = fixture_guard();
    type Key = fn(&Enrollment) -> String;
    let cases: [(&str, Key); 3] = [
        ("source", source_key),
        ("context", context_key),
        ("ledger", ledger_key),
    ];
    for (name, key) in cases {
        let fixture = Fixture::new(name);
        let enrollment = prime(&fixture, 1, false);
        let rows = fixture.private_rows();
        assert_eq!(rows.len(), 3);
        let removed = key(&enrollment);
        let changed = fixture
            .raw_write()
            .execute("DELETE FROM fixture_records WHERE key=?1", [&removed])
            .unwrap();
        assert_eq!(changed, 1, "{name}");
        let expected: Vec<_> = rows
            .iter()
            .filter(|(k, _)| *k != removed)
            .cloned()
            .collect();
        let store = fixture.reopen();
        let seen = store.conversation_observe(&enrollment.conversation, deadline());
        match (name, seen) {
            ("ledger", Err(Error::Conversation(DomainError::Missing))) => {}
            ("source" | "context", Err(Error::Incompatible)) => {}
            (_, other) => panic!("{name}: unexpected {other:?}"),
        }
        let again = store.conversation_enroll(&enrollment, deadline());
        assert!(
            matches!(again, Err(Error::Incompatible)),
            "{name}: {again:?}"
        );
        assert_eq!(fixture.private_rows(), expected, "{name}");
    }
}

#[test]
fn conflicting_enrollments_are_classified_by_the_index_they_share() {
    let _guard = fixture_guard();
    let fixture = Fixture::new("conflict");
    let enrollment = prime(&fixture, 1, false);
    let rows = fixture.private_rows();
    let store = fixture.reopen();
    let shared_source = variant(&enrollment, 70, 1, 71);
    let shared_context = variant(&enrollment, 72, 73, 9);
    let shared_conversation = variant(&enrollment, 7, 74, 75);
    let result = store.conversation_enroll(&shared_source, deadline());
    assert!(
        matches!(result, Err(Error::OwnershipConflict)),
        "{result:?}"
    );
    let result = store.conversation_enroll(&shared_context, deadline());
    assert!(
        matches!(result, Err(Error::OwnershipConflict)),
        "{result:?}"
    );
    let result = store.conversation_enroll(&shared_conversation, deadline());
    assert!(matches!(result, Err(Error::Incompatible)), "{result:?}");
    assert_eq!(fixture.private_rows(), rows);
    // The original enrollment is still intact and idempotent.
    let again = store.conversation_enroll(&enrollment, deadline()).unwrap();
    assert_eq!(again, observe(&store, &enrollment));
}
