//! The seams of workflow definitions. Nothing here names a file, a clock or a hash
//! implementation; adapters are composed by the host.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::DocumentRefusal;
use hf_domain::workflow::tree::DependencyTree;
use hf_domain::workflow::vocab::Freedom;

/// What a backend accepts for reasoning effort.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffortSupport {
    pub accepts_parameter: bool,
    pub levels: Vec<String>,
}

/// The capabilities of one backend that definition validation depends on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendFacts {
    pub name: String,
    pub turn_cap: bool,
    pub model_selection: bool,
    pub caller_approvals: bool,
    pub workflow_caller_approvals: bool,
    pub effort: EffortSupport,
}

/// The only hashing seam: bytes of a definition in canonical form, and their SHA-256.
pub trait DefinitionCodec {
    fn sha256_hex(&self, bytes: &[u8]) -> String;
    /// SHA-256 of the canonical form: sorted keys, compact separators, ASCII escapes.
    fn definition_sha256(&self, definition: &JsonObject) -> String;
    /// The byte length of `json.dumps(value, ensure_ascii=True)`.
    fn ascii_dumps_len(&self, value: &Json) -> usize;
}

/// Backend facts and the grammars each backend owns.
pub trait BackendCatalog {
    fn facts(&self, backend: &str) -> Option<BackendFacts>;
    fn known(&self) -> Vec<String>;
    /// Shape and size bounds of an `allowed_commands` request, or the refusal text.
    fn command_bounds(&self, patterns: &Json) -> Result<Vec<String>, String>;
    /// The backend's own grammar for `allowed_commands` at a freedom, or the refusal text.
    fn check_allowed_commands(
        &self,
        backend: &str,
        patterns: &[String],
        freedom: Freedom,
    ) -> Result<(), String>;
    fn native_subagents_available(&self) -> bool;
}

/// Resolves an absolute path against the host, or returns the host's refusal text.
pub trait PathResolver {
    fn resolve_absolute(&self, path: &str) -> Result<String, String>;
}

pub trait Clock {
    fn unix_seconds(&self) -> f64;
}

/// A source of new workflow ids: 32 lowercase hexadecimal characters, random in production.
pub trait IdSource {
    fn workflow_id(&self) -> String;
}

/// A refusal by the authority that admits a resolved tree to be saved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityRefusal {
    pub message: String,
}

pub trait SaveAuthority {
    fn admit(&self, tree: &DependencyTree) -> Result<(), AuthorityRefusal>;
}

/// A decoded definition and the source name it was stored under.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredDefinition {
    pub name: String,
    pub definition: JsonObject,
}

/// Adapters must preserve the requested source name, enforce source identity/fencing,
/// and return document refusals without partial results. Application callers qualify
/// decoded values before projection, hashing or writing.
/// Stored definitions. `get` reads one source and takes no lock; `list` is in filename
/// order and fails as a whole when any source is refused.
pub trait DefinitionStore {
    type Txn<'a>: DefinitionTxn
    where
        Self: 'a;

    fn get(&self, name: &str) -> Result<Option<StoredDefinition>, DocumentRefusal>;
    fn list(&self) -> Result<Vec<StoredDefinition>, DocumentRefusal>;
    /// Takes the tree lock, then the lock of `name`.
    fn begin(&self, name: &str) -> Result<Self::Txn<'_>, DocumentRefusal>;
}

/// A locked read-modify-write of one definition. Dropping without replace/remove
/// leaves storage unchanged. `list` observes the same locked catalog snapshot;
/// replace/remove enforce the source identity captured by begin.
pub trait DefinitionTxn {
    fn current(&self) -> Option<&StoredDefinition>;
    fn list(&self) -> Result<Vec<StoredDefinition>, DocumentRefusal>;
    fn replace(self, definition: &JsonObject) -> Result<(), DocumentRefusal>;
    fn remove(self) -> Result<(), DocumentRefusal>;
}
