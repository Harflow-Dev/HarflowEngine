//! A `BackendCatalog` over the generated fact table.
use hf_application::workflow::ports::{BackendCatalog, BackendFacts, EffortSupport};
use hf_domain::ctl::text::py_repr;
use hf_domain::workflow::json::Json;
use hf_domain::workflow::vocab::Freedom;

use super::backend_facts::{COMMANDS, FACTS};

pub struct TableCatalog;

const MAX_PATTERNS: usize = 16;
const MAX_PATTERN_BYTES: usize = 256;
const MAX_TOTAL_BYTES: usize = 2048;

fn controlled(pattern: &str) -> bool {
    pattern
        .chars()
        .any(|ch| ch.is_control() || ch == '\u{2028}' || ch == '\u{2029}')
}

/// `command_grants.check_bounds`: shape and size only. Unicode categories other than
/// control, line and paragraph separators are not classified by this double.
fn bounds(patterns: &Json) -> Result<Vec<String>, String> {
    let Json::Array(items) = patterns else {
        return Err("allowed_commands must be a list of strings".to_string());
    };
    if items.len() > MAX_PATTERNS {
        return Err(format!(
            "allowed_commands accepts at most {MAX_PATTERNS} patterns, got {}",
            items.len()
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut total = 0;
    for item in items {
        let Some(pattern) = item.as_str() else {
            return Err("allowed_commands entries must be strings".to_string());
        };
        let shown = py_repr(pattern);
        if pattern.is_empty() {
            return Err("allowed_commands entries must not be empty".to_string());
        }
        if pattern.trim() != pattern {
            return Err(format!(
                "allowed_commands pattern {shown} has leading or trailing whitespace"
            ));
        }
        if controlled(pattern) {
            return Err(format!(
                "allowed_commands pattern {shown} contains a control character"
            ));
        }
        let size = pattern.len();
        if size > MAX_PATTERN_BYTES {
            let head: String = pattern.chars().take(32).collect();
            return Err(format!(
                "allowed_commands pattern {}... is {size} bytes; the limit is {MAX_PATTERN_BYTES}",
                py_repr(&head)
            ));
        }
        if seen.contains(&pattern) {
            return Err(format!("allowed_commands pattern {shown} is duplicated"));
        }
        seen.push(pattern);
        total += size;
    }
    if total > MAX_TOTAL_BYTES {
        return Err(format!(
            "allowed_commands total size is {total} bytes; the limit is {MAX_TOTAL_BYTES}"
        ));
    }
    Ok(seen.into_iter().map(str::to_string).collect())
}

impl BackendCatalog for TableCatalog {
    fn facts(&self, backend: &str) -> Option<BackendFacts> {
        FACTS
            .iter()
            .find(|row| row.name == backend)
            .map(|row| BackendFacts {
                name: row.name.to_string(),
                turn_cap: row.turn_cap,
                model_selection: row.model_selection,
                caller_approvals: row.caller_approvals,
                workflow_caller_approvals: row.workflow_caller_approvals,
                effort: EffortSupport {
                    accepts_parameter: row.accepts_effort,
                    levels: row.levels.iter().map(|level| level.to_string()).collect(),
                },
            })
    }

    fn known(&self) -> Vec<String> {
        FACTS.iter().map(|row| row.name.to_string()).collect()
    }

    fn command_bounds(&self, patterns: &Json) -> Result<Vec<String>, String> {
        bounds(patterns)
    }

    fn check_allowed_commands(
        &self,
        backend: &str,
        patterns: &[String],
        freedom: Freedom,
    ) -> Result<(), String> {
        let row = COMMANDS.iter().find(|row| {
            row.backend == backend
                && row.freedom == freedom.as_str()
                && row
                    .patterns
                    .iter()
                    .copied()
                    .eq(patterns.iter().map(String::as_str))
        });
        // A missing row is a failure of the fixtures, not an acceptance: the refusal names
        // the key so the generator can be told to record it.
        match row {
            None => Err(format!(
                "workflow_support: no recorded allowed_commands outcome for backend={backend:?} \
                 freedom={:?} patterns={patterns:?}; add it to the corpus generator",
                freedom.as_str()
            )),
            Some(found) => match found.refusal {
                Some(message) => Err(message.to_string()),
                None => Ok(()),
            },
        }
    }

    fn native_subagents_available(&self) -> bool {
        false
    }
}
