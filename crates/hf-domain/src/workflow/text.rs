//! Python string predicates that differ from Rust's.

/// Whether Python's `str.isspace` holds for one character.
pub fn is_py_space(ch: char) -> bool {
    let code = ch as u32;
    (0x09..=0x0d).contains(&code)
        || (0x1c..=0x20).contains(&code)
        || code == 0x85
        || code == 0xa0
        || code == 0x1680
        || (0x2000..=0x200a).contains(&code)
        || code == 0x2028
        || code == 0x2029
        || code == 0x202f
        || code == 0x205f
        || code == 0x3000
}

/// Whether `text.strip()` is empty in Python.
pub fn is_blank(text: &str) -> bool {
    text.chars().all(is_py_space)
}

/// Python's `len(text)`: the number of code points.
pub fn length(text: &str) -> usize {
    text.chars().count()
}
