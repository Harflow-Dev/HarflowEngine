//! Canonical JSON of recovery proposals and inventories, and the confirmation token.
//!
//! The form is the reference engine's: sorted keys, compact separators, every non-ASCII
//! code point escaped as `\uXXXX` (surrogate pairs above the basic plane), and a newline
//! only on files. A token is the unpadded base64url of that form; one longer than
//! `TOKEN_BOUND`, or not byte-for-byte the canonical encoding of what it decodes to, is
//! stale.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hf_application::ctl::state_inspect::RecoveryCodecPort;
use hf_domain::ctl::{
    recovery_proposal::{
        EntryBody, EntryStat, FileIdentity, Inventory, InventoryEntry, RecoveryProposal,
        TOKEN_BOUND,
    },
    state_recovery::is_operation_id,
};
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Write as _};

/// A token that does not identify the proposal it claims to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaleToken;

/// Appends `text` as a JSON string with Python's default `ensure_ascii` escaping.
pub fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('"');
}

fn write_identity(out: &mut String, identity: FileIdentity) {
    let _ = write!(out, "[{},{}]", identity.dev, identity.ino);
}

/// The canonical encoding of a proposal, without a newline.
pub fn encode_proposal(proposal: &RecoveryProposal) -> String {
    let mut out = String::from("{\"engine_version\":");
    write_string(&mut out, &proposal.engine_version);
    out.push_str(",\"inventory_sha256\":");
    write_string(&mut out, &proposal.inventory_sha256);
    out.push_str(",\"lock_identity\":");
    match proposal.lock_identity {
        Some(identity) => write_identity(&mut out, identity),
        None => out.push_str("null"),
    }
    out.push_str(",\"operation_id\":");
    write_string(&mut out, &proposal.operation_id);
    out.push_str(",\"prior_journal_sha256\":");
    match &proposal.prior_journal_sha256 {
        Some(digest) => write_string(&mut out, digest),
        None => out.push_str("null"),
    }
    out.push_str(",\"root_identity\":");
    write_identity(&mut out, proposal.root_identity);
    out.push_str(",\"state_root\":");
    write_string(&mut out, &proposal.state_root);
    out.push('}');
    out
}

/// The confirmation token of a proposal.
pub fn encode_token(proposal: &RecoveryProposal) -> String {
    URL_SAFE_NO_PAD.encode(encode_proposal(proposal))
}

/// The proposal a token encodes. Anything over `TOKEN_BOUND` characters, not valid
/// base64url, not a proposal object, or not canonical is `StaleToken`.
pub fn decode_token(token: &str) -> Result<RecoveryProposal, StaleToken> {
    if token.len() > TOKEN_BOUND {
        return Err(StaleToken);
    }
    let bytes = URL_SAFE_NO_PAD.decode(token).map_err(|_| StaleToken)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| StaleToken)?;
    let proposal = proposal_from_value(&value).ok_or(StaleToken)?;
    (encode_token(&proposal) == token)
        .then_some(proposal)
        .ok_or(StaleToken)
}

/// A proposal from its JSON object: exactly the seven fields, correctly typed.
pub fn proposal_from_value(value: &Value) -> Option<RecoveryProposal> {
    let map = value.as_object()?;
    if map.len() != 7 {
        return None;
    }
    let text = |key: &str| map.get(key)?.as_str().map(str::to_string);
    let optional = |key: &str| match map.get(key)? {
        Value::Null => Some(None),
        Value::String(text) => Some(Some(text.clone())),
        _ => None,
    };
    let operation_id = text("operation_id").filter(|id| is_operation_id(id))?;
    let inventory_sha256 = text("inventory_sha256").filter(|digest| is_digest(digest))?;
    let prior_journal_sha256 = optional("prior_journal_sha256")?;
    if prior_journal_sha256
        .as_deref()
        .is_some_and(|d| !is_digest(d))
    {
        return None;
    }
    let lock_identity = match map.get("lock_identity")? {
        Value::Null => None,
        other => Some(identity_from_value(other)?),
    };
    Some(RecoveryProposal {
        state_root: text("state_root")?,
        engine_version: text("engine_version")?,
        root_identity: identity_from_value(map.get("root_identity")?)?,
        lock_identity,
        inventory_sha256,
        operation_id,
        prior_journal_sha256,
    })
}

fn identity_from_value(value: &Value) -> Option<FileIdentity> {
    match value.as_array()?.as_slice() {
        [dev, ino] => Some(FileIdentity {
            dev: dev.as_i64()?,
            ino: ino.as_u64()?,
        }),
        _ => None,
    }
}

fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_entry(out: &mut String, entry: &InventoryEntry) {
    let EntryStat {
        dev,
        ino,
        mode,
        size,
        mtime_ns,
    } = entry.stat;
    let _ = write!(out, "[{dev},{ino},{mode},{size},{mtime_ns}");
    match &entry.body {
        EntryBody::File => {}
        EntryBody::Link(target) => {
            out.push(',');
            write_string(out, target);
        }
        EntryBody::Directory(children) => {
            out.push(',');
            write_children(out, children);
        }
    }
    out.push(']');
}

fn write_children(out: &mut String, children: &BTreeMap<String, InventoryEntry>) {
    out.push('{');
    for (index, (name, entry)) in children.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(out, name);
        out.push(':');
        write_entry(out, entry);
    }
    out.push('}');
}

/// The canonical encoding of an inventory, without a newline.
pub fn encode_inventory(inventory: &Inventory) -> String {
    let mut out = String::new();
    write_children(&mut out, &inventory.entries);
    out
}

/// The `RecoveryCodecPort` of the JSON documents.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonRecoveryCodec;
impl RecoveryCodecPort for JsonRecoveryCodec {
    fn inventory_bytes(&self, inventory: &Inventory) -> Vec<u8> {
        encode_inventory(inventory).into_bytes()
    }
}
