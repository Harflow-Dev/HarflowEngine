//! JSON decoding of the markers and journals older engines leave in a state root.
use super::json_scan;
use hf_application::ctl::state_inspect::StateCodecPort;
use hf_domain::ctl::{
    identity::PRODUCT,
    inspect::{JournalDecode, MarkerDecode, ObservedField, Scalar},
    state_root::StateRoot,
};
use serde_json::{Map, Value};
const OBSERVED_KEYS: [&str; 5] = [
    "product",
    "format",
    "v",
    "product_version",
    "layout_version",
];
const TEXT_LIMIT: usize = 256;
const PHASES: [&str; 5] = ["planned", "prepared", "backed_up", "committed", "completed"];
const RESERVED_NAMES: [&str; 5] = [
    ".",
    "..",
    ".harflow-store.lock",
    ".harflow-recovery.json",
    ".harflow-recovery.json.pending",
];
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonStateCodec;
impl StateCodecPort for JsonStateCodec {
    fn decode_marker(&self, bytes: &[u8]) -> MarkerDecode {
        let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(bytes) else {
            return MarkerDecode::Invalid;
        };
        let ours = map.get("product").and_then(Value::as_str) == Some(PRODUCT);
        let observed = if ours {
            observed_fields(&map)
        } else {
            Vec::new()
        };
        if ours && map.get("format").and_then(Value::as_str) == Some("harflow.store") {
            let recovery_operation_id = map
                .get("recovery_operation_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            MarkerDecode::Store {
                observed,
                recovery_operation_id,
            }
        } else {
            MarkerDecode::Foreign { observed }
        }
    }
    fn decode_journal(&self, bytes: &[u8], root: &StateRoot) -> JournalDecode {
        match json_scan::scan_object(bytes, valid_name) {
            Some(scan) => qualify_journal(&scan, bytes, root.as_str()),
            // Only a failed scan consults the general parser: valid JSON of another
            // shape is unqualified, anything else is unparseable.
            None => match serde_json::from_slice::<Value>(bytes) {
                Ok(_) => JournalDecode::Unqualified,
                Err(_) => JournalDecode::Unparseable,
            },
        }
    }
}
/// Only short text and 64-bit integers are echoed; other values are omitted.
fn observed_fields(map: &Map<String, Value>) -> Vec<ObservedField> {
    let mut fields = Vec::new();
    for key in OBSERVED_KEYS {
        let value = match map.get(key) {
            Some(Value::String(text)) if text.len() <= TEXT_LIMIT => Scalar::Text(text.clone()),
            Some(Value::Number(number)) => match number.as_i64() {
                Some(integer) => Scalar::Int(integer),
                None => continue,
            },
            _ => continue,
        };
        fields.push(ObservedField { key, value });
    }
    fields
}
/// Journals can nest deeply (the inventory), so only the fields that qualify them are
/// read, from a non-recursive scan.
fn qualify_journal(scan: &json_scan::Scan, bytes: &[u8], root: &str) -> JournalDecode {
    qualified(scan, bytes, root).unwrap_or(JournalDecode::Unqualified)
}
type Range = Option<(usize, usize)>;
fn slice(bytes: &[u8], range: Range) -> Option<&[u8]> {
    let (from, to) = range?;
    bytes.get(from..to)
}
fn qualified(scan: &json_scan::Scan, bytes: &[u8], root: &str) -> Option<JournalDecode> {
    let raw = |range: Range| slice(bytes, range);
    let text = |range: Range| raw(range).and_then(json_scan::decode_string);
    let object = |range: Range| raw(range).is_some_and(|v| v.first() == Some(&b'{'));
    let operation = text(scan.top("operation_id")).filter(|id| {
        id.len() == 32
            && id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })?;
    let phase = text(scan.top("phase"))?;
    let backup = format!("{root}.harflow-backup-{operation}");
    let identity = |range: Range| {
        raw(range).map(|v| {
            v.iter()
                .filter(|b| !b.is_ascii_whitespace())
                .copied()
                .collect::<Vec<u8>>()
        })
    };
    let valid = text(scan.top("product")).as_deref() == Some(PRODUCT)
        && text(scan.top("format")).as_deref() == Some("harflow.state-recovery")
        && raw(scan.top("v")) == Some(b"1".as_slice())
        && text(scan.top("state_root")).as_deref() == Some(root)
        && PHASES.contains(&phase.as_str())
        && object(scan.top("proposal"))
        && text(scan.proposal("operation_id")).as_deref() == Some(operation.as_str())
        && text(scan.proposal("state_root")).as_deref() == Some(root)
        && text(scan.top("backup_path")).as_deref() == Some(backup.as_str())
        && identity(scan.top("root_identity")) == identity(scan.proposal("root_identity"))
        && object(scan.top("inventory"))
        && scan.inventory_names_valid();
    Some(if valid {
        JournalDecode::Qualified {
            completed: phase == "completed",
            operation_id: operation,
        }
    } else {
        JournalDecode::Unqualified
    })
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !RESERVED_NAMES.contains(&name)
        && !name.contains('/')
        && !name.contains('\0')
}
