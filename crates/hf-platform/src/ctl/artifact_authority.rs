//! Whether a canonical target may be exposed as an artifact of a task: inside the recorded
//! repository, outside protected state and outside every backend's configuration namespace.
//! Scratch, writable-root and grant sources have no recorded facts here, so a target only
//! they would admit is not authorized. The check reads the environment and symlinks only.
use super::{
    kernel_spelling::{Spelling, kernel_spelling},
    state_root::{home_directory, resolve_state_root},
};
use nix::unistd::User;
use std::{
    cell::OnceCell,
    collections::HashSet,
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

/// Names no artifact may sit under, at any depth of the traversed paths.
const FORBIDDEN: [&str; 7] = [
    ".harflow",
    ".polybridge",
    ".codex",
    ".claude",
    ".vibe",
    ".config",
    ".git",
];
/// Project namespaces of the backends, looked for at the repository and each of its parents.
const PROJECT: [&str; 11] = [
    ".claude",
    ".mcp.json",
    ".gemini",
    ".antigravity",
    ".cursor",
    ".vibe",
    ".agents/skills",
    "opencode.json",
    "opencode.jsonc",
    ".opencode",
    ".codex",
];
const SYMLINK_HOPS: usize = 40;

fn components(path: &Path) -> Vec<OsString> {
    path.components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_os_string()),
            Component::ParentDir => Some(OsString::from("..")),
            _ => None,
        })
        .collect()
}

/// Every lexical component, respelled component and symlink hop of an absolute `path`, and
/// the path it finally names. Each existing component is respelled as the kernel stores it,
/// without following it, so a differently-cased alias of a protected name is recognized.
/// `None` when a lookup fails other than by absence or a symlink chain is not bounded.
fn chain(path: &Path) -> Option<(HashSet<PathBuf>, PathBuf)> {
    let mut pending = components(path);
    let mut current = PathBuf::from("/");
    let mut seen = HashSet::new();
    let mut hops = 0;
    while !pending.is_empty() {
        let part = pending.remove(0);
        if part == ".." {
            current.pop();
            continue;
        }
        current.push(part);
        seen.insert(current.clone());
        if let Spelling::Found(actual) = kernel_spelling(&current, false)? {
            seen.insert(current.with_file_name(actual.file_name()?));
            current = actual;
        }
        seen.insert(current.clone());
        if !std::fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        hops += 1;
        if hops > SYMLINK_HOPS {
            return None;
        }
        let target = std::fs::read_link(&current).ok()?;
        if target.is_absolute() {
            current = PathBuf::from("/");
        } else {
            current.pop();
        }
        let mut next = components(&target);
        next.append(&mut pending);
        pending = next;
    }
    Some((seen, current))
}

/// Python's `Path.expanduser()`: a leading `~` is the home directory, `~name` that account's
/// home through `getpwnam`; an unknown account is a `RuntimeError` there and `None` here.
fn expand_user(text: &str, home: &Path) -> Option<PathBuf> {
    if !text.starts_with('~') {
        return Some(PathBuf::from(text));
    }
    let (first, rest) = text.split_once('/').unwrap_or((text, ""));
    let name = &first[1..];
    let base = if name.is_empty() {
        home.to_path_buf()
    } else {
        User::from_name(name).ok()??.dir
    };
    Some(if rest.is_empty() {
        base
    } else {
        base.join(rest)
    })
}

/// A configured path, read as the pinned `environment_path` reads every backend variable
/// (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `OPENCODE_TEST_HOME`, the four `XDG_*_HOME`, the four
/// `OPENCODE_*` paths, `VIBE_HOME`): `expanduser`, then it must be absolute. `Some(None)`
/// when unset, `None` when set but empty, relative, unknown-account or not text.
fn configured(name: &str, home: &Path) -> Option<Option<PathBuf>> {
    let Some(value) = std::env::var_os(name) else {
        return Some(None);
    };
    let text = value.to_str()?;
    let path = expand_user(text, home)?;
    (!text.is_empty() && path.is_absolute()).then_some(Some(path))
}

fn or_default(name: &str, home: &Path, default: PathBuf) -> Option<PathBuf> {
    Some(configured(name, home)?.unwrap_or(default))
}

/// Why a target cannot be judged. Python's backends build their namespaces in registry
/// order, and what they raise decides how far it travels: a bad configured path is a
/// `ValueError` that makes this one artifact unavailable, but a missing account is a
/// `KeyError` that ends the whole command.
pub(super) enum Refusal {
    Item,
    Command(String),
}

/// The account name of the real uid, or the text Python's `KeyError` would carry.
pub(super) type Account<'a> = &'a dyn Fn() -> Result<String, String>;

/// The configuration namespaces of every backend outside a repository, built in the order
/// Python builds them: claude, codex, opencode (its account lookup after its configured
/// roots), vibe, antigravity, cursor.
fn external(home: &Path, account: Account<'_>) -> Result<Vec<PathBuf>, Refusal> {
    let item = |value: Option<PathBuf>| value.ok_or(Refusal::Item);
    let mut paths = Vec::new();
    let claude = item(or_default("CLAUDE_CONFIG_DIR", home, home.join(".claude")))?;
    let mut claude_json = claude.clone().into_os_string();
    claude_json.push(".json");
    paths.extend([
        home.join(".claude"),
        home.join(".claude.json"),
        claude,
        PathBuf::from(claude_json),
        PathBuf::from("/Library/Application Support/ClaudeCode"),
        PathBuf::from("/etc/claude-code"),
    ]);
    let codex = item(or_default("CODEX_HOME", home, home.join(".codex")))?;
    paths.extend([home.join(".codex"), codex]);
    let opencode_home = item(or_default("OPENCODE_TEST_HOME", home, home.to_path_buf()))?;
    let tails = [".config", ".local/share", ".cache", ".local/state"];
    for base in [home, opencode_home.as_path()] {
        for tail in tails {
            paths.push(base.join(tail).join("opencode"));
        }
    }
    for (name, tail) in [
        ("XDG_CONFIG_HOME", tails[0]),
        ("XDG_DATA_HOME", tails[1]),
        ("XDG_CACHE_HOME", tails[2]),
        ("XDG_STATE_HOME", tails[3]),
    ] {
        // `~` expands against the real home, the default against the test home.
        let base = item(or_default(name, home, opencode_home.join(tail)))?;
        paths.push(base.join("opencode"));
    }
    paths.extend([
        PathBuf::from("/Library/Application Support/opencode"),
        PathBuf::from("/etc/opencode"),
        PathBuf::from("/etc/xdg/opencode"),
    ]);
    paths.extend(managed_preferences(&account().map_err(Refusal::Command)?));
    for name in [
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_DIR",
        "OPENCODE_TUI_CONFIG",
        "OPENCODE_TEST_MANAGED_CONFIG_DIR",
    ] {
        paths.extend(configured(name, home).ok_or(Refusal::Item)?);
    }
    let vibe = item(or_default("VIBE_HOME", home, home.join(".vibe")))?;
    paths.extend([home.join(".vibe"), vibe, home.join(".agents/skills")]);
    paths.extend([home.join(".gemini"), PathBuf::from("/etc/antigravity")]);
    paths.push(home.join(".cursor"));
    Ok(paths)
}

/// The opencode managed-preference files at the pin: the computer-level plist and the
/// plist of the account the process runs as. Nothing else in that directory is looked at.
fn managed_preferences(account: &str) -> Vec<PathBuf> {
    const PLIST: &str = "ai.opencode.managed.plist";
    let directory = Path::new("/Library/Managed Preferences");
    vec![directory.join(PLIST), directory.join(account).join(PLIST)]
}

/// Bridge state: the home defaults and the selected root.
fn state_roots(home: &Path) -> Vec<PathBuf> {
    let mut roots = vec![home.join(".harflow"), home.join(".polybridge")];
    if let Ok(selected) = resolve_state_root(std::env::var_os("HARFLOW_STATE_ROOT")) {
        roots.push(PathBuf::from(selected.as_str()));
    }
    roots
}

/// A root with where it resolves, each resolved once per check.
fn resolved(roots: impl IntoIterator<Item = PathBuf>) -> Option<Vec<(PathBuf, PathBuf)>> {
    roots
        .into_iter()
        .map(|root| chain(&root).map(|(_, final_path)| (root, final_path)))
        .collect()
}

/// A path is inside a root by the root's own spelling or by where it resolves.
fn inside(path: &Path, roots: &[(PathBuf, PathBuf)]) -> bool {
    roots
        .iter()
        .any(|(root, final_path)| path.starts_with(root) || path.starts_with(final_path))
}

/// What does not depend on the repository, resolved once per command and on first need: the
/// bridge's state roots, and the backends' namespaces. Each is kept with where it resolves,
/// and a failure is kept too.
#[derive(Default)]
pub(super) struct Shared {
    states: OnceCell<Option<Vec<(PathBuf, PathBuf)>>>,
    namespaces: OnceCell<Result<Vec<(PathBuf, PathBuf)>, Refusal>>,
}

impl Shared {
    /// The failure that ends the whole command, once an artifact has reached it.
    pub(super) fn command_failure(&self) -> Option<String> {
        match self.namespaces.get() {
            Some(Err(Refusal::Command(message))) => Some(message.clone()),
            _ => None,
        }
    }
}

/// Whether `target` (canonical and absolute) is exposed by the task whose repository is
/// `repo_path`. Anything that cannot be verified is not authorized.
pub(super) fn authorized(
    shared: &Shared,
    account: Account<'_>,
    repo_path: &str,
    target: &Path,
) -> bool {
    check(shared, account, repo_path, target).unwrap_or(false)
}

fn check(shared: &Shared, account: Account<'_>, repo_path: &str, target: &Path) -> Option<bool> {
    let home = home_directory()?;
    let recorded = Path::new(repo_path);
    let repo = match kernel_spelling(recorded, true)? {
        Spelling::Found(repo) => Some(repo),
        Spelling::Missing => None,
    };
    let (mut traversed, _) = chain(target)?;
    traversed.insert(target.to_path_buf());
    let states = shared
        .states
        .get_or_init(|| resolved(state_roots(&home)))
        .as_ref()?;
    if traversed.iter().any(|hop| inside(hop, states)) {
        return Some(false);
    }
    // The hops carry the on-disk spelling, so the names are compared exactly.
    let named = |hop: &PathBuf| {
        hop.components()
            .any(|part| FORBIDDEN.iter().any(|name| part.as_os_str() == *name))
    };
    if traversed.iter().any(named) {
        return Some(false);
    }
    // Only an artifact that gets this far builds the namespaces, as in the reference.
    let namespaces = shared
        .namespaces
        .get_or_init(|| {
            let paths = external(&home, account)?;
            resolved(paths).ok_or(Refusal::Item)
        })
        .as_ref()
        .ok()?;
    let base = repo.as_deref().unwrap_or(recorded);
    let project = base
        .ancestors()
        .flat_map(|parent| PROJECT.iter().map(move |name| parent.join(name)));
    if inside(target, namespaces) || inside(target, &resolved(project)?) {
        return Some(false);
    }
    Some(repo.is_some_and(|repo| target.starts_with(repo)))
}
