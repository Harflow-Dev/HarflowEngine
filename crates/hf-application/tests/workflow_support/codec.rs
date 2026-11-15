//! A `DefinitionCodec` that hashes with FNV-1a over a plain rendering. It is not SHA-256;
//! tests that need Python's digests compose the real codec in hf-host.
use hf_application::workflow::ports::DefinitionCodec;
use hf_domain::workflow::json::{Json, JsonObject};

pub struct FakeCodec;

fn render(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Json::Int(number) => out.push_str(&number.text()),
        Json::Float(number) => out.push_str(&format!("{number:?}")),
        Json::Str(text) => out.push_str(&format!("{text:?}")),
        Json::Array(items) => {
            out.push('[');
            for item in items {
                render(item, out);
                out.push(',');
            }
            out.push(']');
        }
        Json::Object(map) => {
            out.push('{');
            let mut entries: Vec<_> = map.entries().iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            for (key, item) in entries {
                out.push_str(&format!("{key:?}:"));
                render(item, out);
                out.push(',');
            }
            out.push('}');
        }
    }
}

impl DefinitionCodec for FakeCodec {
    fn sha256_hex(&self, bytes: &[u8]) -> String {
        let mut out = String::new();
        for round in 0u64..4 {
            let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ round;
            for byte in bytes {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
            }
            out.push_str(&format!("{hash:016x}"));
        }
        out
    }

    fn definition_sha256(&self, definition: &JsonObject) -> String {
        let mut text = String::new();
        render(&Json::Object(definition.clone()), &mut text);
        self.sha256_hex(text.as_bytes())
    }

    fn ascii_dumps_len(&self, value: &Json) -> usize {
        let mut text = String::new();
        render(value, &mut text);
        text.len()
    }
}
