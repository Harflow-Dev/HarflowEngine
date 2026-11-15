//! Candidate checks that depend on one backend's capabilities.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::Refusal;
use hf_domain::workflow::rule_shape::CandidateRule;

use super::Validator;
use super::candidate::given;
use super::field::{Check, refuse};
use crate::workflow::ports::BackendFacts;

impl Validator<'_> {
    /// `backends.get(candidate.get("backend", ""))`.
    pub(super) fn backend_of(&self, candidate: &JsonObject) -> Check<BackendFacts> {
        let value = candidate.get("backend").cloned().unwrap_or(Json::text(""));
        let found = value.as_str().and_then(|name| self.catalog.facts(name));
        found.ok_or_else(|| {
            let mut known = self.catalog.known();
            known.sort();
            refuse(CandidateRule::UnknownBackend)
                .with(value)
                .with(Json::texts(&known))
        })
    }

    /// `reject_model`, `reject_turn_cap` and `check_reasoning_effort`, first refusal only.
    pub(super) fn capability(&self, candidate: &JsonObject, facts: &BackendFacts) -> Check<()> {
        if let Some(model) = given(candidate, "model")
            && !facts.model_selection
        {
            return Err(named(CandidateRule::NoModelSelection, facts).with(model.clone()));
        }
        if let Some(turns) = given(candidate, "max_turns")
            && !facts.turn_cap
        {
            return Err(named(CandidateRule::NoTurnCap, facts).with(turns.clone()));
        }
        let Some(effort) = given(candidate, "reasoning_effort") else {
            return Ok(());
        };
        if !facts.effort.accepts_parameter {
            return Err(named(CandidateRule::NoEffortControl, facts).with(effort.clone()));
        }
        let accepted = effort
            .as_str()
            .is_some_and(|text| facts.effort.levels.iter().any(|level| level == text));
        if !accepted {
            return Err(named(CandidateRule::EffortNotAccepted, facts)
                .with(effort.clone())
                .with(Json::texts(&facts.effort.levels)));
        }
        Ok(())
    }
}

fn named(rule: CandidateRule, facts: &BackendFacts) -> Refusal {
    refuse(rule).with_text(&facts.name)
}
