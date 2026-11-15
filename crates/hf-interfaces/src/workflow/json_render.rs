//! JSON text as Python's `json.dumps` writes it, in four modes.
use hf_domain::workflow::json::{Json, JsonObject};

use super::py_float;

/// The layout of rendered text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Style {
    /// Sorted keys, `,` and `:` separators, ASCII escapes: the bytes that are hashed.
    Canonical,
    /// Insertion order, `, ` and `: ` separators, ASCII escapes: `json.dumps(value)`.
    Line,
    /// Insertion order, `indent=2`, ASCII escapes.
    Pretty,
    /// Insertion order, `, ` and `: ` separators, `ensure_ascii=False`: what a stored
    /// definition file holds.
    Storage,
}

impl Style {
    fn ascii(self) -> bool {
        self != Style::Storage
    }

    fn separators(self) -> (&'static str, &'static str) {
        match self {
            Style::Canonical => (",", ":"),
            Style::Pretty => (",", ": "),
            Style::Line | Style::Storage => (", ", ": "),
        }
    }
}

fn escape(out: &mut String, text: &str, ascii: bool) {
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
            c if !ascii && c as u32 >= 0x20 => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

struct Writer {
    style: Style,
    out: String,
}

impl Writer {
    fn newline(&mut self, level: usize) {
        if self.style == Style::Pretty {
            self.out.push('\n');
            self.out.push_str(&" ".repeat(2 * level));
        }
    }

    fn items<T>(&mut self, items: &[T], level: usize, mut each: impl FnMut(&mut Self, &T)) {
        let (item, _) = self.style.separators();
        for (index, entry) in items.iter().enumerate() {
            if index > 0 {
                self.out.push_str(item);
            }
            self.newline(level + 1);
            each(self, entry);
        }
    }

    fn value(&mut self, value: &Json, level: usize) {
        match value {
            Json::Null => self.out.push_str("null"),
            Json::Bool(flag) => self.out.push_str(if *flag { "true" } else { "false" }),
            Json::Int(number) => self.out.push_str(&number.text()),
            Json::Float(number) => self.out.push_str(&py_float::repr(*number)),
            Json::Str(text) => escape(&mut self.out, text, self.style.ascii()),
            Json::Array(items) if items.is_empty() => self.out.push_str("[]"),
            Json::Array(items) => {
                self.out.push('[');
                self.items(items, level, |writer, item| writer.value(item, level + 1));
                self.newline(level);
                self.out.push(']');
            }
            Json::Object(map) => self.object(map, level),
        }
    }

    fn object(&mut self, map: &JsonObject, level: usize) {
        if map.is_empty() {
            self.out.push_str("{}");
            return;
        }
        let mut entries: Vec<&(String, Json)> = map.entries().iter().collect();
        if self.style == Style::Canonical {
            entries.sort_by(|left, right| left.0.cmp(&right.0));
        }
        let (_, pair) = self.style.separators();
        self.out.push('{');
        self.items(&entries, level, |writer, entry| {
            escape(&mut writer.out, &entry.0, writer.style.ascii());
            writer.out.push_str(pair);
            writer.value(&entry.1, level + 1);
        });
        self.newline(level);
        self.out.push('}');
    }
}

/// `json.dumps` of a value in the given style.
pub fn render(value: &Json, style: Style) -> String {
    let mut writer = Writer {
        style,
        out: String::new(),
    };
    writer.value(value, 0);
    writer.out
}

/// `json.dumps` of an object in the given style.
pub fn render_object(map: &JsonObject, style: Style) -> String {
    let mut writer = Writer {
        style,
        out: String::new(),
    };
    writer.object(map, 0);
    writer.out
}

/// The bytes a definition hashes: `json.dumps(sort_keys=True, separators=(",", ":"))`.
pub fn canonical_bytes(definition: &JsonObject) -> Vec<u8> {
    render_object(definition, Style::Canonical).into_bytes()
}

/// The byte length of `json.dumps(value, ensure_ascii=True)`.
pub fn ascii_dumps_len(value: &Json) -> usize {
    render(value, Style::Line).len()
}
