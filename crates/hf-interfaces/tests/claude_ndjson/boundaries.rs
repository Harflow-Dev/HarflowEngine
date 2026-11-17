use super::*;

#[test]
fn limit_configuration_rejects_zero_values() {
    let defaults = Limits::default();
    let zero_values = [
        Limits {
            max_line_bytes: 0,
            ..defaults
        },
        Limits {
            max_event_bytes: 0,
            ..defaults
        },
        Limits {
            max_records: 0,
            ..defaults
        },
        Limits {
            max_depth: 0,
            ..defaults
        },
        Limits {
            max_identifier_bytes: 0,
            ..defaults
        },
        Limits {
            max_key_bytes: 0,
            ..defaults
        },
        Limits {
            max_collection_members: 0,
            ..defaults
        },
    ];
    assert_eq!(defaults.validate(), Ok(defaults));
    assert_eq!(
        Limits {
            max_line_bytes: 256,
            ..defaults
        }
        .validate(),
        Ok(Limits {
            max_line_bytes: 256,
            ..defaults
        })
    );
    for zero in zero_values {
        assert_eq!(zero.validate(), Err(BackendError::Budget));
    }
}

#[test]
fn limit_configuration_rejects_values_above_default() {
    let defaults = Limits::default();
    let above_values = [
        Limits {
            max_line_bytes: defaults.max_line_bytes + 1,
            ..defaults
        },
        Limits {
            max_event_bytes: defaults.max_event_bytes + 1,
            ..defaults
        },
        Limits {
            max_records: defaults.max_records + 1,
            ..defaults
        },
        Limits {
            max_depth: defaults.max_depth + 1,
            ..defaults
        },
        Limits {
            max_identifier_bytes: defaults.max_identifier_bytes + 1,
            ..defaults
        },
        Limits {
            max_key_bytes: defaults.max_key_bytes + 1,
            ..defaults
        },
        Limits {
            max_collection_members: defaults.max_collection_members + 1,
            ..defaults
        },
    ];
    for above in above_values {
        assert_eq!(above.validate(), Err(BackendError::Budget));
    }
}

#[test]
fn line_identifier_and_key_limits_accept_exact_boundary() {
    let exact_line = Limits {
        max_line_bytes: INIT.len(),
        ..Limits::default()
    };
    assert!(matches!(
        decoder(exact_line).feed(&line(INIT)).outcome,
        Outcome::Event(_)
    ));
    let short_line = Limits {
        max_line_bytes: INIT.len() - 1,
        ..Limits::default()
    };
    assert_eq!(
        decoder(short_line).feed(&line(INIT)).outcome,
        Outcome::Fault(BackendError::Budget)
    );

    let ascii = br#"{"type":"system","subtype":"init","session_id":"s"}"#;
    let utf8 = r#"{"type":"system","subtype":"init","session_id":"é"}"#.as_bytes();
    let two_bytes = Limits {
        max_identifier_bytes: 2,
        ..Limits::default()
    };
    assert!(matches!(
        decoder(two_bytes).feed(&line(utf8)).outcome,
        Outcome::Event(_)
    ));
    let one_byte = Limits {
        max_identifier_bytes: 1,
        ..Limits::default()
    };
    assert_eq!(
        decoder(one_byte).feed(&line(utf8)).outcome,
        Outcome::Fault(BackendError::Shape)
    );

    let exact_key = Limits {
        max_key_bytes: "session_id".len(),
        ..Limits::default()
    };
    assert!(matches!(
        decoder(exact_key).feed(&line(ascii)).outcome,
        Outcome::Event(_)
    ));
    let short_key = Limits {
        max_key_bytes: "session_id".len() - 1,
        ..Limits::default()
    };
    assert_eq!(
        decoder(short_key).feed(&line(ascii)).outcome,
        Outcome::Fault(BackendError::Budget)
    );
}

fn two_text_records() -> Vec<u8> {
    line(br#"{"type":"assistant","session_id":"s","parent_tool_use_id":"p","message":{"content":[{"type":"text","text":"x"},{"type":"text","text":"yy"}]}}"#)
}

#[test]
fn depth_limit_counts_root_container_as_one() {
    let records = two_text_records();
    let exact = Limits {
        max_depth: 4,
        ..Limits::default()
    };
    assert!(matches!(
        decoder(exact).feed(&records).outcome,
        Outcome::Event(_)
    ));
    let shallow = Limits {
        max_depth: 3,
        ..Limits::default()
    };
    assert_eq!(
        decoder(shallow).feed(&records).outcome,
        Outcome::Fault(BackendError::Budget)
    );
}

#[test]
fn array_collection_limit_is_checked_at_boundary() {
    let exact = Limits {
        max_collection_members: 3,
        ..Limits::default()
    };
    let three = line(br#"{"type":"assistant","session_id":"s","message":{"content":[{"type":"text","text":"x"},{"type":"text","text":"y"},{"type":"text","text":"z"}]}}"#);
    let four = line(br#"{"type":"assistant","session_id":"s","message":{"content":[{"type":"text","text":"x"},{"type":"text","text":"y"},{"type":"text","text":"z"},{"type":"text","text":"w"}]}}"#);
    assert!(matches!(
        decoder(exact).feed(&three).outcome,
        Outcome::Event(_)
    ));
    assert_eq!(
        decoder(exact).feed(&four).outcome,
        Outcome::Fault(BackendError::Budget)
    );
}

#[test]
fn object_collection_limit_is_checked_at_boundary() {
    let exact = Limits {
        max_collection_members: 4,
        ..Limits::default()
    };
    let four =
        line(br#"{"type":"system","subtype":"init","session_id":"s","parent_tool_use_id":"p"}"#);
    let five = line(br#"{"type":"system","subtype":"init","session_id":"s","parent_tool_use_id":"p","extra":null}"#);
    assert!(matches!(
        decoder(exact).feed(&four).outcome,
        Outcome::Event(_)
    ));
    assert_eq!(
        decoder(exact).feed(&five).outcome,
        Outcome::Fault(BackendError::Budget)
    );
}

#[test]
fn record_count_and_repeated_normalized_bytes_are_exactly_bounded() {
    let records = two_text_records();
    let exact_records = Limits {
        max_records: 2,
        ..Limits::default()
    };
    assert_eq!(
        event(decoder(exact_records).feed(&records).outcome).len(),
        2
    );
    let short_records = Limits {
        max_records: 1,
        ..Limits::default()
    };
    assert_eq!(
        decoder(short_records).feed(&records).outcome,
        Outcome::Fault(BackendError::Budget)
    );

    // Both records repeat attempt, session, and parent strings; text bytes count too.
    let exact_bytes = 2 * (ATTEMPT.len() + 1 + 1) + 1 + 2;
    let exact_event = Limits {
        max_event_bytes: exact_bytes,
        ..Limits::default()
    };
    assert_eq!(event(decoder(exact_event).feed(&records).outcome).len(), 2);
    let short_event = Limits {
        max_event_bytes: exact_bytes - 1,
        ..Limits::default()
    };
    assert_eq!(
        decoder(short_event).feed(&records).outcome,
        Outcome::Fault(BackendError::Budget)
    );
}
