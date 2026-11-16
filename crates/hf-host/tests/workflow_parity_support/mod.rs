//! Shared by the workflow parity tests: corpus loading and the production codec.
#![allow(dead_code)]
use std::path::PathBuf;

use hf_application::workflow::ports::DefinitionCodec;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_interfaces::workflow::json_render::{ascii_dumps_len, canonical_bytes};
use hf_interfaces::workflow::json_text::{Limits, parse_with};
use hf_storage::Sha256Stream;

/// The codec a host composes: canonical bytes hashed with SHA-256.
pub struct RealCodec;

impl DefinitionCodec for RealCodec {
    fn sha256_hex(&self, bytes: &[u8]) -> String {
        // The inherent methods of the stream need no trait import. The limit is the
        // stream's own ceiling, so a refusal cannot occur for any definition.
        let Ok(mut stream) = Sha256Stream::with_limit(512 * 1024 * 1024) else {
            return String::new();
        };
        if stream.update(bytes).is_err() {
            return String::new();
        }
        stream
            .finish()
            .map(|digest| digest.as_str().to_string())
            .unwrap_or_default()
    }

    fn definition_sha256(&self, definition: &JsonObject) -> String {
        self.sha256_hex(&canonical_bytes(definition))
    }

    fn ascii_dumps_len(&self, value: &Json) -> usize {
        ascii_dumps_len(value)
    }
}

/// One corpus case: its id and the fields of its fixture entry.
pub struct Case {
    pub id: String,
    pub fields: JsonObject,
}

impl Case {
    pub fn get(&self, key: &str) -> &Json {
        self.fields.get(key).unwrap_or(&Json::Null)
    }

    pub fn input(&self) -> &Json {
        self.get("input")
    }

    pub fn expected(&self) -> &JsonObject {
        match self.get("expected") {
            Json::Object(map) => map,
            _ => panic!("case {} has no expected outcome", self.id),
        }
    }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workflow_r10a")
}

pub fn read_fixture(name: &str) -> Json {
    let text = std::fs::read_to_string(fixtures_dir().join(name)).expect("fixture is readable");
    parse_with(
        &text,
        Limits {
            max_bytes: usize::MAX,
            max_depth: 4096,
        },
    )
    .expect("fixture is valid JSON")
}

/// The cases of a fixture file such as `validate.json`.
pub fn load_cases(name: &str) -> Vec<Case> {
    let Json::Object(document) = read_fixture(name) else {
        panic!("fixture {name} is not an object");
    };
    let Some(Json::Array(items)) = document.get("cases") else {
        panic!("fixture {name} has no cases");
    };
    items
        .iter()
        .map(|item| {
            let Json::Object(fields) = item else {
                panic!("fixture case is not an object");
            };
            let id = fields
                .get("id")
                .and_then(Json::as_str)
                .unwrap_or("?")
                .to_string();
            Case {
                id,
                fields: fields.clone(),
            }
        })
        .collect()
}

/// The resolver table of the corpus: `path -> resolved path or refusal text`.
pub fn resolver_entries() -> Vec<(String, Result<String, String>)> {
    let Json::Object(document) = read_fixture("backend_facts.json") else {
        panic!("backend facts fixture is not an object");
    };
    let Some(Json::Object(table)) = document.get("resolver") else {
        panic!("backend facts fixture has no resolver table");
    };
    table
        .iter()
        .map(|(path, entry)| {
            let entry = entry.as_object().expect("resolver entry is an object");
            let outcome = if entry.get("ok") == Some(&Json::Bool(true)) {
                Ok(entry
                    .get("path")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string())
            } else {
                Err(entry
                    .get("message")
                    .and_then(Json::as_str)
                    .unwrap_or("")
                    .to_string())
            };
            (path.to_string(), outcome)
        })
        .collect()
}
