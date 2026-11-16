//! Python's `repr` of decoded JSON values, as messages interpolate them with `!r`.
use hf_domain::ctl::text::py_repr;
use hf_domain::workflow::json::Json;

use super::py_float;

/// `repr(value)` of the Python object a decoded JSON value becomes.
pub fn repr_of(value: &Json) -> String {
    match value {
        Json::Null => "None".to_string(),
        Json::Bool(true) => "True".to_string(),
        Json::Bool(false) => "False".to_string(),
        Json::Int(number) => number.text(),
        Json::Float(number) => py_float::repr(*number)
            .replace("NaN", "nan")
            .replace("Infinity", "inf"),
        Json::Str(text) => py_repr(text),
        Json::Array(items) => {
            let parts: Vec<String> = items.iter().map(repr_of).collect();
            format!("[{}]", parts.join(", "))
        }
        Json::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_repr(key), repr_of(item)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}
