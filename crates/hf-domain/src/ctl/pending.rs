//! Pending launch discovery as pure policy: the bounds, the text cuts and the projection of
//! one stored launch row into what the pending reads report. Receipts are display evidence,
//! never task status; a qualified ordinary record always wins over a receipt.
use crate::task_read::TaskId;

pub const SCOPE: &str = "prerecord_launch_attempts_and_not_started";
/// The suffix of a receipt filename; a receipt's name stem is its row key.
pub const SUFFIX: &str = ".launch.json";
pub const MAX_ENTRIES: usize = 65_536;
pub const MAX_NAME_BYTES: usize = 8 * 1024 * 1024;
/// The bytes one request may read before its read budget is spent.
pub const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 128 * 1024;
/// Items of a page stay below the response bound by this much; the finished result by
/// `RESULT_RESERVE`.
pub const ITEMS_RESERVE: usize = 8192;
pub const RESULT_RESERVE: usize = 1024;
/// Candidates one page examines, filtered ones included.
pub const MAX_EXAMINED: usize = 100;
/// Bytes charged to the read budget for each receipt read, as the reference charges them.
pub const LAUNCH_READ_CHARGE: u64 = 65_537;
/// The most a receipt may hold; a larger one is refused as `oversize` without being decoded.
pub const LAUNCH_MAX_BYTES: u64 = 65_536;
/// The suffix of an ordinary record's filename.
pub const RECORD_SUFFIX: &str = ".meta.json";
pub const MAX_CURSOR_BYTES: usize = 4096;
pub const MAX_LIMIT: i64 = 100;
pub const DEFAULT_LIMIT: i64 = 50;
const REPO_PATH_BYTES: usize = 4096;
const TEXT_BYTES: usize = 1024;
const NAME_BYTES: usize = 256;
const MAX_LISTED_NAMES: usize = 100;
const MAX_NAMES: usize = 16;

/// The first `max` bytes of `value`, without a character cut in half.
pub fn cut(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// A resource request as the reference validates it: 1 to 16 distinct names of lowercase
/// letters, digits and `. _ : -`, 1 to 64 characters, starting with a letter or digit.
pub fn names_valid(names: &[String]) -> bool {
    let name_ok = |name: &String| {
        !name.is_empty()
            && name.len() <= 64
            && name.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || (index > 0 && b"._:-".contains(&byte))
            })
    };
    let distinct: std::collections::BTreeSet<_> = names.iter().collect();
    (1..=MAX_NAMES).contains(&names.len())
        && names.iter().all(name_ok)
        && distinct.len() == names.len()
}

/// The stored facts of one launch row whose name is a task identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchFacts {
    pub id: String,
    /// An ordinary record has this id.
    pub record: bool,
    /// `None`: the receipt does not qualify as a launch document.
    pub state: Option<String>,
    /// The sanitized `(code, check)` of a refusal.
    pub refusal: Option<(String, String)>,
    pub generation: Option<String>,
    pub title: Option<String>,
    pub backend: Option<String>,
    pub repo_path: Option<String>,
    pub created_at: Option<String>,
    pub not_started_reason: Option<String>,
    pub spawned_by: Option<String>,
    pub root_task_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub chain_root_task_id: Option<String>,
    /// `None` without a stored list (or one that is not a list of strings).
    pub resource_names: Option<Vec<String>>,
    pub has_resources: bool,
    pub has_worktree: bool,
    /// Whether the display text columns are text the wire can carry.
    pub text_valid: bool,
}

/// What a pending launch shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchView {
    pub title: Option<String>,
    pub backend: Option<String>,
    pub repo_path: Option<String>,
    pub created_at: Option<String>,
    pub not_started_reason: Option<String>,
    pub generation: String,
    pub state: String,
    /// Only valid full task identities, in the order of the receipt's fields.
    pub lineage: Vec<(&'static str, String)>,
    pub resource_names: Vec<String>,
    pub metadata_truncated: bool,
    pub cancel_support: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// A qualified ordinary record exists; history is outside this inventory.
    Recorded,
    Pending(Box<LaunchView>),
    Refused {
        code: &'static str,
        check: &'static str,
    },
    /// A spawned or settled receipt without a record.
    Inconsistent,
}

const CODES: [&str; 13] = [
    "format",
    "version",
    "root_mismatch",
    "task_id_mismatch",
    "generation",
    "owner_shape",
    "oversize",
    "field_shape",
    "identity_mismatch",
    "not_regular",
    "task_unavailable",
    "withdrawal_claim",
    "withdrawal_proof",
];
const CHECKS: [&str; 15] = [
    "format",
    "version",
    "root_mismatch",
    "task_id_mismatch",
    "generation",
    "owner_shape",
    "oversize",
    "field_shape",
    "identity_mismatch",
    "not_regular",
    "source_changed",
    "withdrawal_claim",
    "withdrawal_proof",
    "resource_names",
    "display_text_encoding",
];

/// A stored refusal as the wire spells it; anything the reference never produces reads as a
/// malformed field.
pub fn refusal_of(code: &str, check: &str) -> (&'static str, &'static str) {
    let pick = |value: &str, set: &[&'static str]| set.iter().copied().find(|name| *name == value);
    match (pick(code, &CODES), pick(check, &CHECKS)) {
        (Some(code), Some(check)) => (code, check),
        _ => ("field_shape", "field_shape"),
    }
}

/// The reason text the reference gives for a launch document check.
pub fn document_reason(check: &str) -> &'static str {
    match check {
        "format" => "Incompatible task launch format",
        "version" => "Incompatible task launch version",
        "root_mismatch" => "Task launch root differs from selected root",
        "task_id_mismatch" => "Task launch task ID differs from source name",
        "generation" => "Invalid launch generation",
        "owner_shape" => "Malformed launch owner identity",
        "oversize" => "Task launch exceeds 65536 bytes",
        "identity_mismatch" => {
            "Task launch directory binding differs from selected tasks directory"
        }
        "not_regular" => "Task launch source is not a regular file",
        "source_changed" => "Task launch changed while reading",
        "withdrawal_claim" => "Withdrawal no-spawn claim cannot be qualified",
        "withdrawal_proof" => "Withdrawal terminal proof cannot be qualified",
        _ => "Malformed task launch field",
    }
}

fn refused(code: &str, check: &str) -> LaunchOutcome {
    let (code, check) = refusal_of(code, check);
    LaunchOutcome::Refused { code, check }
}

fn truncated(facts: &LaunchFacts, names: &[String]) -> bool {
    let over = |value: &Option<String>, max: usize| value.as_ref().is_some_and(|v| v.len() > max);
    names.len() > MAX_LISTED_NAMES
        || names.iter().any(|name| name.len() > NAME_BYTES)
        || over(&facts.title, TEXT_BYTES)
        || over(&facts.backend, TEXT_BYTES)
        || over(&facts.repo_path, REPO_PATH_BYTES)
        || over(&facts.created_at, TEXT_BYTES)
        || over(&facts.not_started_reason, TEXT_BYTES)
}

fn view(facts: &LaunchFacts, state: &str) -> LaunchView {
    let names = facts.resource_names.clone().unwrap_or_default();
    let text = |value: &Option<String>, max| value.as_ref().map(|v| cut(v, max));
    let lineage = [
        ("spawned_by", &facts.spawned_by),
        ("root_task_id", &facts.root_task_id),
        ("parent_task_id", &facts.parent_task_id),
        ("chain_root_task_id", &facts.chain_root_task_id),
    ]
    .into_iter()
    .filter_map(|(key, value)| {
        let value = value.as_ref()?;
        TaskId::new(value.clone()).ok()?;
        Some((key, value.clone()))
    })
    .collect();
    LaunchView {
        title: text(&facts.title, TEXT_BYTES),
        backend: text(&facts.backend, TEXT_BYTES),
        repo_path: text(&facts.repo_path, REPO_PATH_BYTES),
        created_at: text(&facts.created_at, TEXT_BYTES),
        not_started_reason: text(&facts.not_started_reason, TEXT_BYTES),
        generation: facts.generation.clone().unwrap_or_default(),
        state: state.to_owned(),
        lineage,
        resource_names: names
            .iter()
            .take(MAX_LISTED_NAMES)
            .map(|name| cut(name, NAME_BYTES))
            .collect(),
        metadata_truncated: truncated(facts, &names),
        cancel_support: state != "not_started" && (facts.has_resources || facts.has_worktree),
    }
}

/// The disposition of a launch row, in the reference's order: a record wins, then an
/// unqualified document is refused, then spawned and settled receipts are inconsistent
/// without a record, then display text and resource names are checked.
pub fn outcome(facts: &LaunchFacts) -> LaunchOutcome {
    if facts.record {
        return LaunchOutcome::Recorded;
    }
    let stored = facts
        .refusal
        .as_ref()
        .map(|(code, check)| (code.as_str(), check.as_str()));
    let Some(state) = facts.state.as_deref() else {
        let (code, check) = stored.unwrap_or(("field_shape", "field_shape"));
        return refused(code, check);
    };
    if state == "spawned" || state == "settled" {
        return LaunchOutcome::Inconsistent;
    }
    if !facts.text_valid {
        return refused("field_shape", "display_text_encoding");
    }
    if let Some((code, check)) = stored {
        return refused(code, check);
    }
    let names_ok = !facts.has_resources || facts.resource_names.as_deref().is_some_and(names_valid);
    if !names_ok {
        return refused("field_shape", "resource_names");
    }
    LaunchOutcome::Pending(Box::new(view(facts, state)))
}
