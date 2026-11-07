//! The argparse-compatible token scan shared by every command: options with and without a
//! value, `--opt=value`, unique prefixes, the `--` terminator, positionals and integers.
use super::cli::UsageError;

#[derive(Clone, Copy)]
pub(super) struct Opt {
    pub flag: &'static str,
    pub value: bool,
}

pub(super) const JSON: Opt = Opt {
    flag: "--json",
    value: false,
};
const HELP: Opt = Opt {
    flag: "--help",
    value: false,
};

#[derive(Default)]
pub(super) struct Scan {
    pub json: bool,
    pub help: bool,
    flags: Vec<&'static str>,
    values: Vec<(&'static str, String)>,
    /// Bare tokens, in order.
    pub positionals: Vec<String>,
    /// Tokens that matched no option.
    pub extras: Vec<String>,
}

impl Scan {
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }
    /// The last value given for the option.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|entry| entry.0 == name)
            .map(|entry| entry.1.as_str())
    }
}

pub(super) fn error(program: &str, message: String) -> UsageError {
    UsageError {
        program: program.to_string(),
        message,
    }
}

/// Exact match, or a unique prefix of a long option.
fn resolve(options: &[Opt], name: &str) -> Option<Opt> {
    let full = format!("--{name}");
    if let Some(exact) = options.iter().find(|option| option.flag == full) {
        return Some(*exact);
    }
    let mut found = options
        .iter()
        .filter(|option| !name.is_empty() && option.flag.starts_with(&full));
    match (found.next(), found.next()) {
        (Some(only), None) => Some(*only),
        _ => None,
    }
}

fn negative_number(token: &str) -> bool {
    token.strip_prefix('-').is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    })
}

fn option_like(token: &str) -> bool {
    token.starts_with('-') && token != "-" && !negative_number(token)
}

pub(super) fn scan(program: &str, options: &[Opt], args: &[String]) -> Result<Scan, UsageError> {
    let mut all = vec![JSON, HELP];
    all.extend_from_slice(options);
    let mut result = Scan::default();
    let mut bare_only = false;
    let mut index = 0;
    while let Some(token) = args.get(index) {
        index += 1;
        if bare_only || !token.starts_with('-') || token == "-" {
            result.positionals.push(token.clone());
        } else if token == "--" {
            bare_only = true;
        } else if token == "-h" {
            result.help = true;
            return Ok(result);
        } else if let Some(body) = token.strip_prefix("--") {
            let (name, explicit) = match body.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (body, None),
            };
            let Some(option) = resolve(&all, name) else {
                result.extras.push(token.clone());
                continue;
            };
            if !option.value {
                if let Some(value) = explicit {
                    let message = format!(
                        "argument {}: ignored explicit argument '{value}'",
                        option.flag
                    );
                    return Err(error(program, message));
                }
                match option.flag {
                    "--help" => {
                        result.help = true;
                        return Ok(result);
                    }
                    "--json" => result.json = true,
                    other => result.flags.push(other),
                }
                continue;
            }
            let value = match explicit {
                Some(value) => value.to_string(),
                None => match args.get(index) {
                    Some(next) if !option_like(next) => {
                        index += 1;
                        next.clone()
                    }
                    _ => {
                        let message = format!("argument {}: expected one argument", option.flag);
                        return Err(error(program, message));
                    }
                },
            };
            result.values.push((option.flag, value));
        } else {
            result.extras.push(token.clone());
        }
    }
    Ok(result)
}

/// Python's `int(text)` for the spellings an option value can take.
pub(super) fn py_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let well_formed = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'_');
    if !well_formed {
        return None;
    }
    let magnitude: i64 = digits.replace('_', "").parse().ok()?;
    Some(if negative { -magnitude } else { magnitude })
}

pub(super) fn int_option(program: &str, flag: &str, text: &str) -> Result<i64, UsageError> {
    py_int(text).ok_or_else(|| {
        error(
            program,
            format!("argument {flag}: invalid int value: {}", py_repr(text)),
        )
    })
}

pub(super) use hf_domain::ctl::text::py_repr;

/// `--since`: a non-negative integer and one unit of `smhdw`, as whole seconds.
pub(super) fn duration_seconds(raw: &str) -> Option<i64> {
    let text = raw.trim();
    let unit = text.chars().last()?;
    let digits = &text[..text.len() - unit.len_utf8()];
    let scale: i64 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        'w' => 604_800,
        _ => return None,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok()?.checked_mul(scale)
}
