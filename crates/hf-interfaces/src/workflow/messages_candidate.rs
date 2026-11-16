//! The text of candidate, on-demand and resource refusals.
use hf_domain::workflow::rule_shape::{CandidateRule, OnDemandRule};

use super::messages::Args;

pub(super) fn on_demand_message(rule: OnDemandRule, a: &Args<'_>) -> String {
    let id = a.text(0);
    match rule {
        OnDemandRule::OnDemandNotArray => "on_demand_nodes must be an array".to_string(),
        OnDemandRule::OnDemandKeys => "On-demand nodes accept only id, title, instructions, call_condition, agent, freedom, network, session_mode, execution_mode and resources".to_string(),
        OnDemandRule::OnDemandInstructions => {
            format!("On-demand instructions must be nonempty text: {id}")
        }
        OnDemandRule::OnDemandTitleOrCondition => {
            format!("On-demand title and call_condition must be text: {id}")
        }
        OnDemandRule::OnDemandFreedomOrNetwork => {
            format!("Invalid on-demand freedom or network: {id}")
        }
        OnDemandRule::OnDemandSessionMode => format!("Invalid on-demand session_mode: {id}"),
        OnDemandRule::OnDemandExecutionMode => format!("Invalid on-demand execution_mode: {id}"),
        OnDemandRule::ResourcesOnlyAgents => format!("Only agent nodes accept resources: {id}"),
        OnDemandRule::ResourceNamesNotList => "resource names must be a list of names".to_string(),
        OnDemandRule::ResourceNamesSingleString => {
            "resource names must be a list of names, not a single string".to_string()
        }
        OnDemandRule::ResourceCount => format!("request 1 to 16 resource names, got {id}"),
        OnDemandRule::ResourceNameInvalid => format!(
            "invalid resource name {}: use lowercase letters, digits and . _ : - (1 to 64 characters, starting with a letter or digit)",
            a.repr(0)
        ),
        OnDemandRule::ResourceDuplicate => "duplicate resource names in one request".to_string(),
        OnDemandRule::ResourcePreferSubagent => {
            format!("prefer_subagent nodes cannot declare resources: {id}")
        }
        OnDemandRule::ResourceSubset => {
            format!("Node resources must be a subset of run resources: {id}")
        }
    }
}

pub(super) fn candidate_message(rule: CandidateRule, a: &Args<'_>) -> String {
    match rule {
        CandidateRule::CandidateNotObject => "Agent must be an object".to_string(),
        CandidateRule::UnknownAgentSettings => format!("Unknown agent settings: {}", a.repr(0)),
        CandidateRule::InvalidConversationProfile => {
            "conversation_profile must be native or shipped-medium35-high".to_string()
        }
        CandidateRule::FallbacksNotList => "Fallbacks must be an ordered array".to_string(),
        CandidateRule::UnknownBackend => {
            format!(
                "unknown backend {}; expected one of {}",
                a.repr(0),
                a.repr(1)
            )
        }
        CandidateRule::CallerApprovalsNotBool => "caller_approvals must be a boolean".to_string(),
        CandidateRule::CallerWriteApprovalsNotBool => {
            "caller_write_approvals must be a boolean".to_string()
        }
        CandidateRule::CallerWriteNeedsApprovals => {
            "caller_write_approvals requires caller_approvals".to_string()
        }
        CandidateRule::GroupReportNotBool => "group_report_output must be a boolean".to_string(),
        CandidateRule::GroupReportNotAllowedHere => {
            "group_report_output belongs to workflow agent nodes, not orchestrators or builders"
                .to_string()
        }
        CandidateRule::GroupReportWithApprovals => {
            "group_report_output cannot be combined with caller approvals".to_string()
        }
        CandidateRule::GitHttpsRewriteNotBool => "git_https_rewrite must be a boolean".to_string(),
        CandidateRule::ObserveReposNotAllowedHere => {
            "observe_repos belongs to workflow agent nodes".to_string()
        }
        CandidateRule::ObserveReposBounds => {
            "observe_repos must be a list of at most seven absolute paths".to_string()
        }
        CandidateRule::ObserveReposAbsolute => {
            "observe_repos must contain absolute paths".to_string()
        }
        CandidateRule::ObserveReposResolve | CandidateRule::AllowedCommandsBounds => a.text(0),
        CandidateRule::AllowedCommandsNotAllowedHere => {
            "allowed_commands belongs to workflow agent nodes, not orchestrators or builders"
                .to_string()
        }
        CandidateRule::CallerApprovalsUnqualified => {
            "Caller approvals require a qualified uncapped backend".to_string()
        }
        CandidateRule::ModelNotText => "Model must be a nonempty string".to_string(),
        CandidateRule::BackgroundIdleInvalid => {
            "background_idle_seconds must be a finite positive number no greater than 21600"
                .to_string()
        }
        CandidateRule::InvalidReasoningEffort => "Invalid reasoning_effort".to_string(),
        CandidateRule::NoModelSelection => format!(
            "the {} CLI has no model selection flag, so model={} cannot be honoured; omit it, or use a backend whose capabilities report supports_model_selection",
            a.text(0),
            a.repr(1)
        ),
        CandidateRule::NoTurnCap => format!(
            "the {} CLI has no turn cap, so max_turns={} cannot be honoured; omit it, or use a backend whose capabilities report supports_turn_cap",
            a.text(0),
            a.text(1)
        ),
        CandidateRule::NoEffortControl => format!(
            "the {} CLI has no reasoning effort control, so reasoning_effort={} cannot be honoured; omit it, or use a backend whose capabilities report reasoning_effort.accepts_parameter",
            a.text(0),
            a.repr(1)
        ),
        CandidateRule::EffortNotAccepted => format!(
            "the {} CLI does not accept reasoning_effort={}; it accepts {}",
            a.text(0),
            a.repr(1),
            a.repr(2)
        ),
        CandidateRule::FallbacksNotFlat => "Fallbacks must be an ordered flat list".to_string(),
    }
}
