#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;

fn staged_files() -> BTreeMap<String, StagedFile> {
    FRESH_FILES
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            (
                name.to_owned(),
                StagedFile {
                    identity: id(50 + index as u64),
                    size: 100 + index as u64,
                    mtime_ns: 10 + index as i64,
                    sha256: format!("{:064x}", index + 1),
                },
            )
        })
        .collect()
}

fn completed_journal(with_stage: bool) -> RecoveryJournal {
    let mut value = oracle_journal();
    value.phase = JournalPhase::Completed;
    value.backup_identity = Some(id(20));
    value.data_identity = Some(id(21));
    value.result = Some(RecoveryResult {
        operation_id: OP.into(),
        state_root: ROOT.into(),
        backup_path: value.backup_path.clone(),
    });
    if with_stage {
        value.staging = Some(id(45));
        value.staged = Some(staged_files());
    }
    value
}

#[test]
fn old_precommit_journal_fixture_remains_byte_compatible() {
    assert_eq!(encode_journal(&oracle_journal()).unwrap(), ORACLE);
    assert_eq!(decode_journal(ORACLE).unwrap(), oracle_journal());
}

#[test]
fn backed_up_journal_accepts_only_complete_optional_staging_metadata() {
    let mut value = oracle_journal();
    value.phase = JournalPhase::BackedUp;
    value.backup_identity = Some(id(20));
    value.data_identity = Some(id(21));
    value.staging = Some(id(45));
    value.staged = Some(staged_files());
    let encoded = encode_journal(&value).unwrap();
    assert_eq!(decode_journal(&encoded).unwrap(), value);

    let mut missing_file = value.clone();
    missing_file.staged.as_mut().unwrap().remove(FRESH_FILES[0]);
    assert_eq!(encode_journal(&missing_file), Err(JournalError::Invalid));
    let mut wrong_hash = value;
    wrong_hash
        .staged
        .as_mut()
        .unwrap()
        .get_mut(FRESH_FILES[0])
        .unwrap()
        .sha256 = "z".repeat(64);
    assert_eq!(encode_journal(&wrong_hash), Err(JournalError::Invalid));
}

#[test]
fn completed_journals_roundtrip_with_and_without_staging_metadata() {
    for value in [completed_journal(false), completed_journal(true)] {
        let encoded = encode_journal(&value).unwrap();
        assert_eq!(decode_journal(&encoded).unwrap(), value);
    }
    let mut missing_result = completed_journal(true);
    missing_result.result = None;
    assert_eq!(encode_journal(&missing_result), Err(JournalError::Invalid));
    let mut mismatched_result = completed_journal(false);
    mismatched_result.result.as_mut().unwrap().operation_id = "f".repeat(32);
    assert_eq!(
        encode_journal(&mismatched_result),
        Err(JournalError::Invalid)
    );
}

#[test]
fn completed_result_codec_is_canonical_and_rejects_wrong_bindings_and_fields() {
    let result = completed_journal(false).result.unwrap();
    let bytes = encode_result(&result).unwrap();
    assert!(bytes.ends_with(b"}\n"));
    assert_eq!(decode_result(&bytes).unwrap(), result);
    let unknown = replace_once(
        &bytes,
        "\"status\":\"completed\"",
        "\"status\":\"completed\",\"extra\":1",
    );
    assert_eq!(
        decode_result(&unknown),
        Err(JournalError::UnsupportedExtension)
    );
    let wrong_backup = replace_once(
        &bytes,
        &format!("\"backup_id\":\"{OP}\""),
        "\"backup_id\":\"ffffffffffffffffffffffffffffffff\"",
    );
    assert_eq!(decode_result(&wrong_backup), Err(JournalError::Invalid));
    assert_eq!(
        decode_result(&vec![b'x'; JOURNAL_BOUND + 1]),
        Err(JournalError::TooLarge)
    );
}

#[test]
fn worst_phase_bound_includes_staging_metadata_previous_bytes_and_newline() {
    let mut value = completed_journal(true);
    let mut exact = None;
    for pad in 0..4 {
        value.created_at = format!("2026-11-16T00:00:00Z{}", "x".repeat(pad));
        let mut low = 0usize;
        let mut high = JOURNAL_BOUND;
        while low <= high {
            let mid = low + (high - low) / 2;
            value.previous_journal_bytes = Some(vec![0xff; mid]);
            match encode_journal(&value) {
                Ok(bytes) if bytes.len() == JOURNAL_BOUND => {
                    exact = Some((value.clone(), bytes));
                    break;
                }
                Ok(bytes) => {
                    assert!(bytes.len() < JOURNAL_BOUND);
                    low = mid + 1;
                }
                Err(JournalError::TooLarge) => high = mid.saturating_sub(1),
                Err(error) => panic!("unexpected encode error: {error:?}"),
            }
        }
        if exact.is_some() {
            break;
        }
    }
    let (at_limit, bytes) = exact.expect("worst phase must reach the complete byte bound");
    assert_eq!(bytes.len(), JOURNAL_BOUND);
    assert!(bytes.ends_with(b"}\n"));
    assert_eq!(decode_journal(&bytes).unwrap(), at_limit);
    assert_eq!(
        decode_journal(&[bytes.as_slice(), b" "].concat()),
        Err(JournalError::TooLarge)
    );
}
