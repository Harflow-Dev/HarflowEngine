//! Candidate rules.
use hf_domain::workflow::refusal::Rule::Candidate as C;
use hf_domain::workflow::rule_shape::CandidateRule as CR;

use super::Entry;

pub const CANDIDATE: &[Entry] = &[
    (C(CR::CandidateNotObject), "[]", "Agent must be an object"),
    (
        C(CR::UnknownAgentSettings),
        r#"[["x", "y"]]"#,
        "Unknown agent settings: ['x', 'y']",
    ),
    (
        C(CR::InvalidConversationProfile),
        "[]",
        "conversation_profile must be native or shipped-medium35-high",
    ),
    (
        C(CR::FallbacksNotList),
        "[]",
        "Fallbacks must be an ordered array",
    ),
    (
        C(CR::UnknownBackend),
        r#"["nope", ["claude", "codex"]]"#,
        "unknown backend 'nope'; expected one of ['claude', 'codex']",
    ),
    (
        C(CR::CallerApprovalsNotBool),
        "[]",
        "caller_approvals must be a boolean",
    ),
    (
        C(CR::CallerWriteApprovalsNotBool),
        "[]",
        "caller_write_approvals must be a boolean",
    ),
    (
        C(CR::CallerWriteNeedsApprovals),
        "[]",
        "caller_write_approvals requires caller_approvals",
    ),
    (
        C(CR::GroupReportNotBool),
        "[]",
        "group_report_output must be a boolean",
    ),
    (
        C(CR::GroupReportNotAllowedHere),
        "[]",
        "group_report_output belongs to workflow agent nodes, not orchestrators or builders",
    ),
    (
        C(CR::GroupReportWithApprovals),
        "[]",
        "group_report_output cannot be combined with caller approvals",
    ),
    (
        C(CR::GitHttpsRewriteNotBool),
        "[]",
        "git_https_rewrite must be a boolean",
    ),
    (
        C(CR::ObserveReposNotAllowedHere),
        "[]",
        "observe_repos belongs to workflow agent nodes",
    ),
    (
        C(CR::ObserveReposBounds),
        "[]",
        "observe_repos must be a list of at most seven absolute paths",
    ),
    (
        C(CR::ObserveReposAbsolute),
        "[]",
        "observe_repos must contain absolute paths",
    ),
    (
        C(CR::ObserveReposResolve),
        r#"["host refusal"]"#,
        "host refusal",
    ),
    (
        C(CR::AllowedCommandsNotAllowedHere),
        "[]",
        "allowed_commands belongs to workflow agent nodes, not orchestrators or builders",
    ),
    (
        C(CR::AllowedCommandsBounds),
        r#"["bounds refusal"]"#,
        "bounds refusal",
    ),
    (
        C(CR::CallerApprovalsUnqualified),
        "[]",
        "Caller approvals require a qualified uncapped backend",
    ),
    (C(CR::ModelNotText), "[]", "Model must be a nonempty string"),
    (
        C(CR::BackgroundIdleInvalid),
        "[]",
        "background_idle_seconds must be a finite positive number no greater than 21600",
    ),
    (
        C(CR::InvalidReasoningEffort),
        "[]",
        "Invalid reasoning_effort",
    ),
    (
        C(CR::NoModelSelection),
        r#"["vibe", "m"]"#,
        "the vibe CLI has no model selection flag, so model='m' cannot be honoured; omit it, or use a backend whose capabilities report supports_model_selection",
    ),
    (
        C(CR::NoTurnCap),
        r#"["codex", 5]"#,
        "the codex CLI has no turn cap, so max_turns=5 cannot be honoured; omit it, or use a backend whose capabilities report supports_turn_cap",
    ),
    (
        C(CR::NoEffortControl),
        r#"["cursor", "high"]"#,
        "the cursor CLI has no reasoning effort control, so reasoning_effort='high' cannot be honoured; omit it, or use a backend whose capabilities report reasoning_effort.accepts_parameter",
    ),
    (
        C(CR::EffortNotAccepted),
        r#"["antigravity", "xhigh", ["low", "medium", "high"]]"#,
        "the antigravity CLI does not accept reasoning_effort='xhigh'; it accepts ['low', 'medium', 'high']",
    ),
    (
        C(CR::FallbacksNotFlat),
        "[]",
        "Fallbacks must be an ordered flat list",
    ),
];
