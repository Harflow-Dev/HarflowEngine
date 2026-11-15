//! Small field rules shared by the validators. Each follows the Python expression named in
//! its documentation, including its treatment of booleans, floats and absent keys.
use hf_domain::workflow::json::{Json, JsonInt, JsonObject};
use hf_domain::workflow::name::is_identifier;
use hf_domain::workflow::refusal::{Refusal, Rule};
use hf_domain::workflow::rule_shape::{DefinitionRule, NodeRule};

pub type Check<T> = Result<T, Refusal>;

pub fn refuse(rule: impl Into<Rule>) -> Refusal {
    Refusal::new(rule)
}

pub fn refuse_for(rule: impl Into<Rule>, id: &str) -> Refusal {
    Refusal::new(rule).with_text(id)
}

/// `_positive(value, field, maximum)`: an integer, not a boolean, from 1 to `maximum`.
/// An absent key reads as `default`.
pub fn positive(source: &JsonObject, field: &str, default: i64, maximum: i64) -> Check<Json> {
    let value = source.get(field).cloned().unwrap_or(Json::int(default));
    let in_range = value
        .as_int()
        .and_then(JsonInt::to_i64)
        .is_some_and(|number| (1..=maximum).contains(&number));
    if in_range {
        Ok(value)
    } else {
        Err(refuse(DefinitionRule::IntegerOutOfRange)
            .with_text(field)
            .with(Json::int(maximum)))
    }
}

/// `_identifier(value)`.
pub fn identifier(value: Option<&Json>) -> Check<String> {
    value
        .and_then(Json::as_str)
        .filter(|text| is_identifier(text))
        .map(str::to_string)
        .ok_or_else(|| refuse(NodeRule::InvalidIdentifier))
}

/// Whether `value` is a string equal to one of `allowed`.
pub fn is_one_of(value: Option<&Json>, allowed: &[&str]) -> bool {
    value
        .and_then(Json::as_str)
        .is_some_and(|text| allowed.contains(&text))
}

/// The value as an owned object, or `None`.
pub fn object(value: Option<&Json>) -> Option<JsonObject> {
    value.and_then(Json::as_object).cloned()
}

/// `timeout_seconds`: absent or null, or a nonnegative integer that is not a boolean.
pub fn timeout_valid(source: &JsonObject) -> bool {
    match source.get("timeout_seconds") {
        None | Some(Json::Null) => true,
        Some(Json::Int(number)) => !number.is_negative(),
        Some(_) => false,
    }
}

/// `type(value) in {bool, NoneType}` for an optional `network` flag.
pub fn network_valid(source: &JsonObject) -> bool {
    matches!(
        source.get("network"),
        None | Some(Json::Null | Json::Bool(_))
    )
}
