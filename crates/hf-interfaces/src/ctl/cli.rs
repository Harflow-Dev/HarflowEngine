//! argparse-compatible parsing for the implemented paths. Every other path is an
//! invalid choice. Accepts `--opt=value`, a `--` terminator and unique prefixes.
pub const PROGRAM: &str = "harflow-ctl";
pub const VERSION_PROGRAM: &str = "harflow-ctl version-info";
pub const COMMANDS: [&str; 2] = ["version-info", "state-inspect"];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageError {
    pub program: String,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    Help(Option<&'static str>),
    VersionInfo { json: bool },
    StateInspect { json: bool },
}
#[derive(Default)]
struct Scan {
    json: bool,
    help: bool,
    extras: Vec<String>,
}
fn error(program: &str, message: String) -> UsageError {
    UsageError {
        program: program.to_string(),
        message,
    }
}
/// Exact match, or a unique prefix of a long option.
fn resolve(flags: &[&'static str], name: &str) -> Option<&'static str> {
    let full = format!("--{name}");
    if let Some(exact) = flags.iter().find(|flag| **flag == full) {
        return Some(exact);
    }
    let mut found = flags
        .iter()
        .filter(|flag| !name.is_empty() && flag.starts_with(&full));
    match (found.next(), found.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}
fn scan(program: &str, flags: &[&'static str], args: &[String]) -> Result<Scan, UsageError> {
    let mut result = Scan::default();
    let mut positional = false;
    for token in args {
        if positional || !token.starts_with('-') || token == "-" {
            result.extras.push(token.clone());
        } else if token == "--" {
            positional = true;
        } else if token == "-h" {
            result.help = true;
            return Ok(result);
        } else if let Some(body) = token.strip_prefix("--") {
            let (name, value) = match body.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (body, None),
            };
            match resolve(flags, name) {
                Some(flag) => {
                    if let Some(value) = value {
                        let message =
                            format!("argument {flag}: ignored explicit argument '{value}'");
                        return Err(error(program, message));
                    }
                    if flag == "--help" {
                        result.help = true;
                        return Ok(result);
                    }
                    result.json = true;
                }
                None => result.extras.push(token.clone()),
            }
        } else {
            result.extras.push(token.clone());
        }
    }
    Ok(result)
}
fn unrecognized(program: &str, extras: &[String]) -> UsageError {
    error(
        program,
        format!("unrecognized arguments: {}", extras.join(" ")),
    )
}
/// `harflow-ctl version-info ...`: the direct entry, which has its own parser.
pub fn parse_version_info(args: &[String]) -> Result<Parsed, UsageError> {
    let parsed = scan(VERSION_PROGRAM, &["--json", "--help"], args)?;
    if parsed.help {
        return Ok(Parsed::Help(Some("version-info")));
    }
    if !parsed.extras.is_empty() {
        return Err(unrecognized(VERSION_PROGRAM, &parsed.extras));
    }
    Ok(Parsed::VersionInfo { json: parsed.json })
}
pub fn parse_ctl(argv: &[String]) -> Result<Parsed, UsageError> {
    let mut extras = Vec::new();
    let mut index = 0;
    let mut command = None;
    while let Some(token) = argv.get(index) {
        index += 1;
        if token == "--" {
            command = argv.get(index);
            index += 1;
            break;
        }
        if !token.starts_with('-') || token == "-" {
            command = Some(token);
            break;
        }
        let long = token.strip_prefix("--");
        let name = long.map(|body| body.split_once('=').map_or(body, |pair| pair.0));
        if token == "-h" || name.and_then(|name| resolve(&["--help"], name)).is_some() {
            return Ok(Parsed::Help(None));
        }
        extras.push(token.clone());
    }
    let Some(command) = command else {
        let message = "the following arguments are required: command".to_string();
        return Err(error(PROGRAM, message));
    };
    let rest = argv.get(index..).unwrap_or_default();
    let (name, program) = match command.as_str() {
        "version-info" => ("version-info", VERSION_PROGRAM),
        "state-inspect" => ("state-inspect", "harflow-ctl state-inspect"),
        other => {
            let choices = COMMANDS.map(|name| format!("'{name}'")).join(", ");
            let message =
                format!("argument command: invalid choice: '{other}' (choose from {choices})");
            return Err(error(PROGRAM, message));
        }
    };
    let parsed = scan(program, &["--json", "--help"], rest)?;
    if parsed.help {
        return Ok(Parsed::Help(Some(name)));
    }
    extras.extend(parsed.extras);
    if !extras.is_empty() {
        return Err(unrecognized(PROGRAM, &extras));
    }
    Ok(if name == "version-info" {
        Parsed::VersionInfo { json: parsed.json }
    } else {
        Parsed::StateInspect { json: parsed.json }
    })
}
pub fn help_text(command: Option<&str>) -> String {
    match command {
        Some("version-info") => "usage: harflow-ctl version-info [-h] [--json]\n".to_string(),
        Some(_) => "usage: harflow-ctl state-inspect [-h] [--json]\n".to_string(),
        None => concat!(
            "usage: harflow-ctl [-h] command ...\n\n",
            "commands:\n",
            "  version-info   report side-effect-free Harflow v1 compatibility\n",
            "  state-inspect  inspect state without mutation\n\n",
            "State root: HARFLOW_STATE_ROOT, otherwise the home directory's .harflow.\n",
        )
        .to_string(),
    }
}
