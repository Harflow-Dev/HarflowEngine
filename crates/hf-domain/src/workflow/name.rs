//! The name grammars of workflows, graph identifiers and resources. All are ASCII.

fn within(text: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&text.len())
}

fn head_and_tail(text: &str, head: fn(u8) -> bool, tail: fn(u8) -> bool) -> bool {
    let mut bytes = text.bytes();
    bytes.next().is_some_and(head) && bytes.all(tail)
}

fn alnum(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

fn lower_alnum(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit()
}

/// A node, connection or group identifier: 1 to 100 letters, digits, `.`, `-` or `_`,
/// starting with a letter or digit.
pub fn is_identifier(text: &str) -> bool {
    within(text, 1, 100)
        && head_and_tail(text, alnum, |byte| {
            alnum(byte) || byte == b'_' || byte == b'.' || byte == b'-'
        })
}

/// A workflow name: an identifier that may also contain spaces, except at the end.
pub fn is_workflow_name(text: &str) -> bool {
    within(text, 1, 100)
        && !text.ends_with(' ')
        && head_and_tail(text, alnum, |byte| {
            alnum(byte) || byte == b'_' || byte == b'.' || byte == b'-' || byte == b' '
        })
}

/// A resource name: 1 to 64 lowercase letters, digits and `. _ : -`, starting with a
/// letter or digit.
pub fn is_resource_name(text: &str) -> bool {
    within(text, 1, 64)
        && head_and_tail(text, lower_alnum, |byte| {
            lower_alnum(byte) || byte == b'.' || byte == b'_' || byte == b':' || byte == b'-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammars() {
        assert!(is_identifier("a.b-c_d"));
        assert!(!is_identifier(""));
        assert!(!is_identifier("-a"));
        assert!(!is_identifier("a\n"));
        assert!(!is_identifier(&"a".repeat(101)));
        assert!(is_identifier(&"a".repeat(100)));
        assert!(is_workflow_name("my flow"));
        assert!(!is_workflow_name("my flow "));
        assert!(!is_workflow_name(" x"));
        assert!(is_resource_name("db:main.1"));
        assert!(!is_resource_name("DB"));
        assert!(!is_identifier("é"));
    }
}
