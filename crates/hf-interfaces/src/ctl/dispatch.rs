//! Maps one parsed invocation to stdout, stderr and an exit code.
use super::{
    cli::{self, Parsed, UsageError},
    render,
};
use hf_domain::ctl::{
    identity::{CTL_FORMAT, HostIdentity, PRODUCT_VERSION, PROGRAM, VERSION_INFO_FORMAT},
    inspect::StateInspection,
    state_root::{InvalidStateRoot, StateRoot},
};
use serde_json::json;
/// Everything with an effect, supplied by the composition root.
pub trait CtlServices {
    fn resolve_state_root(&self) -> Result<StateRoot, InvalidStateRoot>;
    fn host_identity(&self) -> HostIdentity;
    fn inspect(&self, root: &StateRoot) -> StateInspection;
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}
impl Outcome {
    fn ok(stdout: String) -> Self {
        Self {
            stdout,
            ..Self::default()
        }
    }
}
fn usage(format: &str, json_requested: bool, error: &UsageError) -> Outcome {
    let stdout = if json_requested {
        render::line(&render::usage_error(format, &error.message))
    } else {
        String::new()
    };
    Outcome {
        stdout,
        stderr: format!("{}: error: {}\n", error.program, error.message),
        code: 2,
    }
}
fn has_flag(args: &[String], names: &[&str]) -> bool {
    args.iter().any(|arg| names.contains(&arg.as_str()))
}
pub fn dispatch(argv: &[String], services: &dyn CtlServices) -> Outcome {
    if argv.first().map(String::as_str) == Some("version-info") {
        let rest = argv.get(1..).unwrap_or_default();
        let json_requested = has_flag(rest, &["--json"]);
        return match cli::parse_version_info(rest) {
            Ok(parsed) => run(parsed, services, VERSION_INFO_FORMAT, json_requested),
            Err(error) => usage(VERSION_INFO_FORMAT, json_requested, &error),
        };
    }
    let json_requested = has_flag(argv, &["--json"]);
    let help_requested = has_flag(argv, &["--help", "-h"]);
    if !help_requested && let Err(invalid) = services.resolve_state_root() {
        let error = UsageError {
            program: cli::PROGRAM.to_string(),
            message: invalid.message.to_string(),
        };
        return usage(CTL_FORMAT, json_requested, &error);
    }
    match cli::parse_ctl(argv) {
        Ok(parsed) => run(parsed, services, CTL_FORMAT, json_requested),
        Err(error) => usage(CTL_FORMAT, json_requested, &error),
    }
}
fn run(parsed: Parsed, services: &dyn CtlServices, format: &str, json_requested: bool) -> Outcome {
    match parsed {
        Parsed::Help(command) => Outcome::ok(cli::help_text(command)),
        Parsed::VersionInfo { json } => version_info(services, json, format, json_requested),
        Parsed::StateInspect { json } => match services.resolve_state_root() {
            Ok(root) => {
                let result = render::inspection(&services.inspect(&root));
                Outcome::ok(if json {
                    render::line(&render::stamp(CTL_FORMAT, json!({"result": result})))
                } else {
                    render::pretty(&result)
                })
            }
            Err(invalid) => root_error(invalid, format, json_requested),
        },
    }
}
fn version_info(services: &dyn CtlServices, json: bool, format: &str, requested: bool) -> Outcome {
    let root = match services.resolve_state_root() {
        Ok(root) => root,
        Err(invalid) => return root_error(invalid, format, requested),
    };
    if json {
        let document = render::version_info(&root, &services.host_identity());
        Outcome::ok(render::line(&document))
    } else {
        Outcome::ok(format!("{PROGRAM} {PRODUCT_VERSION}\n"))
    }
}
fn root_error(invalid: InvalidStateRoot, format: &str, json_requested: bool) -> Outcome {
    let program = if format == VERSION_INFO_FORMAT {
        cli::VERSION_PROGRAM
    } else {
        cli::PROGRAM
    };
    let error = UsageError {
        program: program.to_string(),
        message: invalid.message.to_string(),
    };
    usage(format, json_requested, &error)
}
