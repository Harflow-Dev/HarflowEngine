//! Rules on the Git preparation a node asks for. Nothing here touches a repository.
use super::json::{Json, JsonObject};
use super::refusal::Refusal;
use super::rule_shape::GitRule;

/// A request for Git preparation. `None` stands for an absent or null value.
#[derive(Clone, Copy, Debug)]
pub struct GitRequest<'a> {
    pub freedom: &'a str,
    pub branch: Option<&'a Json>,
    pub base: Option<&'a Json>,
    pub fetch: Option<&'a Json>,
    pub fetch_required: Option<&'a Json>,
    pub worktree: bool,
    pub continuation: bool,
}

/// The checked request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitPlan {
    pub branch: Option<String>,
    pub base: Option<String>,
    pub fetch: Option<Vec<String>>,
    pub fetch_required: bool,
}

fn optional<'a>(node: &'a JsonObject, key: &str) -> Option<&'a Json> {
    node.get(key).filter(|value| !value.is_null())
}

impl<'a> GitRequest<'a> {
    /// The request a node carries; `freedom` defaults to `write_in_repo` when absent.
    pub fn from_node(node: &'a JsonObject) -> Self {
        Self {
            freedom: node
                .get("freedom")
                .and_then(Json::as_str)
                .unwrap_or("write_in_repo"),
            branch: optional(node, "branch"),
            base: optional(node, "base"),
            fetch: optional(node, "fetch"),
            fetch_required: node.get("fetch_required"),
            worktree: false,
            continuation: false,
        }
    }
}

fn refuse(rule: GitRule) -> Refusal {
    Refusal::new(rule).with(Json::Null)
}

fn literal_name(value: &Json, label: &str) -> Result<String, Refusal> {
    let valid = value.as_str().filter(|text| {
        let mut bytes = text.bytes();
        bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"_/-".contains(&byte))
            && text
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
    });
    valid
        .map(str::to_string)
        .ok_or_else(|| refuse(GitRule::LiteralName).with_text(label))
}

fn parse_base(value: &Json) -> Result<(String, String), Refusal> {
    let text = literal_name(value, "base")?;
    match text.split_once('/') {
        Some((remote, branch)) if !remote.is_empty() && !branch.is_empty() => Ok((
            literal_name(&Json::text(remote), "base remote")?,
            literal_name(&Json::text(branch), "base branch")?,
        )),
        _ => Err(refuse(GitRule::BaseForm)),
    }
}

fn parse_fetch(value: Option<&Json>) -> Result<Option<Vec<String>>, Refusal> {
    match value {
        None | Some(Json::Bool(false)) => Ok(None),
        Some(Json::Bool(true)) => Ok(Some(vec_of("<default>"))),
        Some(Json::Array(items)) if items.is_empty() => Err(refuse(GitRule::FetchEmpty)),
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| literal_name(item, "fetch remote"))
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(refuse(GitRule::FetchForm)),
    }
}

fn vec_of(text: &str) -> Vec<String> {
    Vec::from([text.to_string()])
}

/// `dispatch_git.validate_request`. Refusals carry the `<prefix>: ` of the node as their
/// first detail, `null` for none, and the label of a literal name second.
pub fn validate_request(request: &GitRequest<'_>) -> Result<GitPlan, Refusal> {
    let fetch_required = match request.fetch_required {
        None => false,
        Some(Json::Bool(flag)) => *flag,
        Some(_) => return Err(refuse(GitRule::FetchRequiredNotBool)),
    };
    let fetch = parse_fetch(request.fetch)?;
    if fetch_required && fetch.is_none() {
        return Err(refuse(GitRule::FetchRequiredNeedsFetch));
    }
    if request.branch.is_none() != request.base.is_none() {
        return Err(refuse(GitRule::BranchBaseTogether));
    }
    let mut branch = None;
    let mut base = None;
    if let (Some(branch_value), Some(base_value)) = (request.branch, request.base) {
        if request.continuation {
            return Err(refuse(GitRule::BranchContinuation));
        }
        if request.freedom == "read_only" {
            return Err(refuse(GitRule::BranchReadOnly));
        }
        if request.worktree {
            return Err(refuse(GitRule::BranchWorktree));
        }
        branch = Some(literal_name(branch_value, "branch")?);
        parse_base(base_value)?;
        base = base_value.as_str().map(str::to_string);
    }
    Ok(GitPlan {
        branch,
        base,
        fetch,
        fetch_required,
    })
}

/// `dispatch_git.validate_node`: the request of a node, refusals prefixed by its id.
pub fn validate_node(node: &JsonObject, node_id: &str) -> Result<GitPlan, Refusal> {
    validate_request(&GitRequest::from_node(node)).map_err(|mut refusal| {
        if let Some(slot) = refusal.detail.first_mut() {
            *slot = Json::text(node_id);
        }
        refusal
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn node(entries: Vec<(&str, Json)>) -> JsonObject {
        JsonObject::from_entries(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    #[test]
    fn git_rules_refuse_with_prefix() {
        let bad = node(vec![("branch", Json::text("x"))]);
        let refusal = validate_node(&bad, "n1").unwrap_err();
        assert_eq!(refusal.detail[0], Json::text("n1"));
        assert_eq!(refusal.rule, GitRule::BranchBaseTogether.into());
        let good = node(vec![
            ("branch", Json::text("feature/x")),
            ("base", Json::text("origin/develop")),
            ("fetch", Json::Bool(true)),
        ]);
        let plan = validate_node(&good, "n1").unwrap();
        assert_eq!(plan.base.as_deref(), Some("origin/develop"));
        assert_eq!(plan.fetch, Some(vec_of("<default>")));
    }
}
