//! Values of the history reads (`task-events`, `task-prompt`, `task-artifacts`): the log
//! object, the stored prompt and its bounded view, and artifact references.
use super::caller::TERMINAL_STATUSES;
use crate::event_read::{CaptureState, LogFacts, LogState};

impl LogState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Open => "open",
            Self::Unknown => "unknown",
            Self::Incomplete => "incomplete",
            Self::Settling => "settling",
            Self::Final => "final",
        }
    }
}

impl CaptureState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::LostWithOwner => "lost_with_owner",
            Self::ObservedTerminal => "observed_terminal",
            Self::Unknown => "unknown",
        }
    }
}

/// The lifecycle evidence of a task's event log, judged from the task record and the
/// stored stream row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogObject {
    pub state: LogState,
    pub task_status: String,
    pub output_drained: bool,
    pub capture: CaptureState,
    pub final_seq: Option<u64>,
}

impl LogObject {
    /// A record-derived termination is authoritative: observed terminal exactly when the
    /// status is a terminal name with an exit code. Otherwise only a stored live or lost
    /// capture passes through from the stream row; anything else is unknown. Grace is the
    /// stored boolean of the stream row, read here and nowhere else.
    pub fn derive(status: &str, exit_code: Option<i64>, drained: bool, stream: &LogFacts) -> Self {
        let capture = if exit_code.is_some() && TERMINAL_STATUSES.contains(&status) {
            CaptureState::ObservedTerminal
        } else {
            match stream.capture {
                CaptureState::Live => CaptureState::Live,
                CaptureState::LostWithOwner => CaptureState::LostWithOwner,
                _ => CaptureState::Unknown,
            }
        };
        let facts = LogFacts {
            task_status: Some(status.to_string()),
            available: stream.available,
            capture,
            output_drained: Some(drained),
            eof_finished_seq: stream.eof_finished_seq,
            within_grace: stream.within_grace,
        };
        let state = facts.state();
        let final_seq = if state == LogState::Final {
            stream.eof_finished_seq
        } else {
            None
        };
        Self {
            state,
            task_status: status.to_string(),
            output_drained: drained,
            capture,
            final_seq,
        }
    }

    /// A stable text of every field, for the change token.
    pub fn canonical(&self) -> String {
        let mut text = String::from(self.state.as_str());
        for part in [self.task_status.as_str(), self.capture.as_str()] {
            text.push('|');
            text.push_str(part);
        }
        text.push('|');
        text.push_str(if self.output_drained { "1" } else { "0" });
        text.push('|');
        if let Some(seq) = self.final_seq {
            text.push_str(&seq.to_string());
        }
        text
    }
}

/// The sidecar digest a prompt source name binds, when it is `<task>.prompt.<64 hex>.txt`.
pub fn sidecar_digest<'a>(task: &str, name: &'a str) -> Option<&'a str> {
    let rest = name.strip_prefix(task)?.strip_prefix(".prompt.")?;
    let digest = rest.strip_suffix(".txt")?;
    let lower_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    (digest.len() == 64 && digest.bytes().all(lower_hex)).then_some(digest)
}

/// What storage found of a sidecar. A blob over 4 MiB is not hashed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptBlob {
    Absent,
    /// The whole blob, whose digest equals the one in the source name.
    Verified(Vec<u8>),
    Mismatch,
    Unverified {
        prefix: Vec<u8>,
        total: u64,
    },
}

/// The stored prompt of a task as storage reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredPrompt {
    pub title: Option<String>,
    /// The whole text of an inline prompt.
    pub inline: Option<String>,
    pub preview: Option<String>,
    /// Whether the source name is the task-bound sidecar name.
    pub bound: bool,
    pub blob: PromptBlob,
}

/// The `prompt` object of the wire result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptView {
    pub state: &'static str,
    pub text: Option<String>,
    pub returned_bytes: usize,
    pub total_bytes: Option<u64>,
    pub truncated: bool,
    pub complete: bool,
    pub integrity: &'static str,
}

/// Text whose UTF-8 encoding is at most `max_bytes`, and whether it was cut. The byte prefix
/// drops only an incomplete trailing character; invalid bytes become U+FFFD, which can be
/// longer than the byte it replaces, so the decoded text is cut again on a character boundary.
pub fn cut(data: &[u8], max_bytes: usize) -> (String, bool) {
    let cut = data.len() > max_bytes;
    let mut kept = &data[..data.len().min(max_bytes)];
    if cut {
        for back in 1..=kept.len().min(4) {
            let lead = kept[kept.len() - back];
            if lead & 0xC0 == 0x80 {
                continue;
            }
            let need = if lead < 0x80 {
                1
            } else if lead < 0xE0 {
                2
            } else if lead < 0xF0 {
                3
            } else {
                4
            };
            if need > back {
                kept = &kept[..kept.len() - back];
            }
            break;
        }
    }
    let text = String::from_utf8_lossy(kept).into_owned();
    let mut used = 0usize;
    for (index, character) in text.char_indices() {
        used += character.len_utf8();
        if used > max_bytes {
            return (text[..index].to_string(), true);
        }
    }
    (text, cut)
}

/// The bounded, integrity-reported view of a stored prompt.
pub fn view_prompt(stored: &StoredPrompt, max_bytes: usize) -> PromptView {
    let mut state = "available";
    let mut integrity = "not_applicable";
    let mut total: Option<u64> = None;
    let mut full = true;
    let data: Option<Vec<u8>> = if let Some(text) = &stored.inline {
        total = Some(text.len() as u64);
        Some(text.clone().into_bytes())
    } else if !stored.bound {
        state = "unavailable";
        None
    } else {
        match &stored.blob {
            PromptBlob::Absent => {
                state = "preview_only";
                full = false;
                Some(stored.preview.clone().unwrap_or_default().into_bytes())
            }
            PromptBlob::Verified(bytes) => {
                total = Some(bytes.len() as u64);
                integrity = "verified";
                Some(bytes.clone())
            }
            PromptBlob::Mismatch => {
                state = "integrity_failed";
                integrity = "failed";
                None
            }
            PromptBlob::Unverified {
                prefix,
                total: size,
            } => {
                total = Some(*size);
                integrity = "unverified";
                Some(prefix.clone())
            }
        }
    };
    let Some(data) = data else {
        return PromptView {
            state,
            text: None,
            returned_bytes: 0,
            total_bytes: None,
            truncated: false,
            complete: false,
            integrity,
        };
    };
    let (text, truncated) = cut(&data, max_bytes);
    let returned = text.len();
    let complete = full && !truncated && total.is_none_or(|size| size == returned as u64);
    PromptView {
        state,
        text: Some(text),
        returned_bytes: returned,
        total_bytes: total,
        truncated,
        complete,
        integrity,
    }
}

/// A file observation: what a reference recorded and what a stat shows now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub mtime_ns: i64,
}

/// One registered reference to a file, metadata only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactItem {
    pub artifact_id: String,
    pub path: String,
    pub kind: String,
    pub note: String,
    pub registered_at: String,
    /// How the reference was registered: `provenance.method`.
    pub method: String,
    pub registered: Observation,
    /// The tool-use id and recorded directory of an observed backend plan.
    pub plan: Option<PlanEvidence>,
}

/// Provenance of an `observed_backend_plan` reference: the tool call that wrote the plan
/// and, when recorded, the directory it lives in with that directory's identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanEvidence {
    pub tool_use_id: String,
    pub directory: Option<PlanDirectory>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanDirectory {
    pub path: String,
    pub device: u64,
    pub inode: u64,
}

/// A task's manifest: its header state, revision and item count (read up to 101).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestHead {
    pub header_valid: bool,
    pub revision: String,
    pub count: usize,
}

/// A manifest holds at most this many items; more is unreadable.
pub const MANIFEST_ITEMS_MAX: usize = 100;

impl ManifestHead {
    pub fn readable(&self) -> bool {
        self.header_valid && self.count <= MANIFEST_ITEMS_MAX
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Available,
    Changed,
    Deleted,
    Unavailable,
}

impl Availability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Changed => "changed",
            Self::Deleted => "deleted",
            Self::Unavailable => "unavailable",
        }
    }
}

/// `artifact_summary` of a task document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtifactSummary {
    pub count: usize,
    pub available: bool,
    pub detail_ref: bool,
}
