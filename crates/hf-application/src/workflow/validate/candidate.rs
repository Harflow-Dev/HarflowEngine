//! An agent candidate: its settings, its fallbacks and the checks that need no backend.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::observe::{MAX_STORED, is_absolute};
use hf_domain::workflow::rule_shape::CandidateRule;
use hf_domain::workflow::text::is_blank;
use hf_domain::workflow::vocab::EFFORTS;

use super::Validator;
use super::field::{Check, positive, refuse};

const KEYS: [&str; 13] = [
    "backend",
    "model",
    "reasoning_effort",
    "max_turns",
    "background_idle_seconds",
    "caller_approvals",
    "caller_write_approvals",
    "allowed_commands",
    "observe_repos",
    "git_https_rewrite",
    "group_report_output",
    "fallbacks",
    "conversation_profile",
];

/// The longest idle bound, in seconds.
const MAX_IDLE_SECONDS: f64 = 21600.0;

/// A present, non-null value: Python's `candidate.get(key) is not None`.
pub(super) fn given<'a>(candidate: &'a JsonObject, key: &str) -> Option<&'a Json> {
    candidate.get(key).filter(|value| !value.is_null())
}

fn flag(candidate: &JsonObject, key: &str) -> bool {
    candidate.get(key).and_then(Json::as_bool).unwrap_or(false)
}

impl Validator<'_> {
    /// `_candidate(raw, grants)`: a normalized copy, or the first refusal.
    pub fn candidate(&self, raw: &Json, grants: bool) -> Check<JsonObject> {
        let Json::Object(source) = raw else {
            return Err(refuse(CandidateRule::CandidateNotObject));
        };
        let mut candidate = source.clone();
        let mut unknown: Vec<String> = candidate
            .keys()
            .filter(|key| !KEYS.contains(key))
            .map(str::to_string)
            .collect();
        if !unknown.is_empty() {
            unknown.sort();
            return Err(refuse(CandidateRule::UnknownAgentSettings).with(Json::texts(&unknown)));
        }
        if let Some(profile) = candidate.get("conversation_profile")
            && !matches!(profile.as_str(), Some("native" | "shipped-medium35-high"))
        {
            return Err(refuse(CandidateRule::InvalidConversationProfile));
        }
        if candidate
            .get("fallbacks")
            .is_some_and(|value| value.as_array().is_none())
        {
            return Err(refuse(CandidateRule::FallbacksNotList));
        }
        let facts = self.backend_of(&candidate)?;
        self.approvals_and_flags(&mut candidate, grants)?;
        self.observe_and_commands(&mut candidate, grants)?;
        self.limits_and_capabilities(&mut candidate, &facts)?;
        self.fallbacks(&mut candidate, grants)?;
        Ok(candidate)
    }

    fn approvals_and_flags(&self, candidate: &mut JsonObject, grants: bool) -> Check<()> {
        let approvals = match candidate.get("caller_approvals") {
            None => false,
            Some(Json::Bool(value)) => *value,
            Some(_) => return Err(refuse(CandidateRule::CallerApprovalsNotBool)),
        };
        candidate.insert("caller_approvals", Json::Bool(approvals));
        if let Some(value) = candidate.get("caller_write_approvals") {
            let Json::Bool(write) = value else {
                return Err(refuse(CandidateRule::CallerWriteApprovalsNotBool));
            };
            if *write && !approvals {
                return Err(refuse(CandidateRule::CallerWriteNeedsApprovals));
            }
            if !*write {
                candidate.remove("caller_write_approvals");
            }
        }
        if let Some(value) = candidate.get("group_report_output") {
            let Json::Bool(report) = value else {
                return Err(refuse(CandidateRule::GroupReportNotBool));
            };
            if *report && !grants {
                return Err(refuse(CandidateRule::GroupReportNotAllowedHere));
            }
            if *report && (approvals || flag(candidate, "caller_write_approvals")) {
                return Err(refuse(CandidateRule::GroupReportWithApprovals));
            }
        }
        if candidate
            .get("git_https_rewrite")
            .is_some_and(|value| value.as_bool().is_none())
        {
            return Err(refuse(CandidateRule::GitHttpsRewriteNotBool));
        }
        Ok(())
    }

    fn observe_and_commands(&self, candidate: &mut JsonObject, grants: bool) -> Check<()> {
        if let Some(value) = candidate.get("observe_repos").cloned() {
            if !grants {
                return Err(refuse(CandidateRule::ObserveReposNotAllowedHere));
            }
            let resolved = self.observe_repos(&value)?;
            candidate.insert("observe_repos", Json::texts(&resolved));
        }
        if let Some(value) = candidate.get("allowed_commands").cloned() {
            if !grants {
                return Err(refuse(CandidateRule::AllowedCommandsNotAllowedHere));
            }
            let patterns = self.catalog.command_bounds(&value).map_err(|reason| {
                refuse(CandidateRule::AllowedCommandsBounds).with_text(&reason)
            })?;
            if patterns.is_empty() {
                candidate.remove("allowed_commands");
            } else {
                candidate.insert("allowed_commands", Json::texts(&patterns));
            }
        }
        Ok(())
    }

    /// `validate_observe_repos`: absolute paths, resolved and without duplicates.
    fn observe_repos(&self, value: &Json) -> Check<Vec<String>> {
        let items: &[Json] = match value {
            Json::Null => return Ok(Vec::new()),
            Json::Array(items) if items.len() <= MAX_STORED => items,
            _ => return Err(refuse(CandidateRule::ObserveReposBounds)),
        };
        let mut paths = Vec::new();
        for item in items {
            match item.as_str() {
                Some(path) if is_absolute(path) => paths.push(path),
                _ => return Err(refuse(CandidateRule::ObserveReposAbsolute)),
            }
        }
        let mut resolved: Vec<String> = Vec::new();
        for path in paths {
            let found = self
                .resolver
                .resolve_absolute(path)
                .map_err(|reason| refuse(CandidateRule::ObserveReposResolve).with_text(&reason))?;
            if !resolved.contains(&found) {
                resolved.push(found);
            }
        }
        Ok(resolved)
    }

    fn limits_and_capabilities(
        &self,
        candidate: &mut JsonObject,
        facts: &crate::workflow::ports::BackendFacts,
    ) -> Check<()> {
        let approvals = flag(candidate, "caller_approvals");
        if approvals
            && (!facts.caller_approvals
                || !facts.workflow_caller_approvals
                || given(candidate, "max_turns").is_some())
        {
            return Err(refuse(CandidateRule::CallerApprovalsUnqualified));
        }
        if given(candidate, "max_turns").is_none() && facts.turn_cap && !approvals {
            candidate.insert("max_turns", Json::int(100));
        }
        if let Some(model) = given(candidate, "model")
            && model.as_str().is_none_or(is_blank)
        {
            return Err(refuse(CandidateRule::ModelNotText));
        }
        if let Some(idle) = given(candidate, "background_idle_seconds").cloned() {
            candidate.insert("background_idle_seconds", Json::Float(idle_seconds(&idle)?));
        }
        if given(candidate, "max_turns").is_some() {
            positive(candidate, "max_turns", 100, 10000)?;
        }
        if let Some(effort) = given(candidate, "reasoning_effort")
            && !effort.as_str().is_some_and(|text| EFFORTS.contains(&text))
        {
            return Err(refuse(CandidateRule::InvalidReasoningEffort));
        }
        match self.capability(candidate, facts) {
            Err(reason) if !has_fallbacks(candidate) => Err(reason),
            _ => Ok(()),
        }
    }

    fn fallbacks(&self, candidate: &mut JsonObject, grants: bool) -> Check<()> {
        let items = candidate
            .get("fallbacks")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        let mut normalized = Vec::new();
        for item in &items {
            normalized.push(self.candidate(item, grants)?);
        }
        let nested = normalized.iter().any(has_fallbacks);
        candidate.insert(
            "fallbacks",
            Json::Array(normalized.into_iter().map(Json::Object).collect()),
        );
        if nested {
            return Err(refuse(CandidateRule::FallbacksNotFlat));
        }
        Ok(())
    }
}

fn has_fallbacks(candidate: &JsonObject) -> bool {
    candidate
        .get("fallbacks")
        .and_then(Json::as_array)
        .is_some_and(|items| !items.is_empty())
}

/// `background_idle.validate`: a finite number above zero and at most six hours, as a float.
fn idle_seconds(value: &Json) -> Check<f64> {
    let number = match value {
        Json::Int(number) => number.to_float(),
        Json::Float(number) => Some(*number),
        _ => None,
    };
    number
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0 && *seconds <= MAX_IDLE_SECONDS)
        .ok_or_else(|| refuse(CandidateRule::BackgroundIdleInvalid))
}
