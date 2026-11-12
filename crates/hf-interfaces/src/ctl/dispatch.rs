//! Maps one parsed invocation to stdout, stderr and an exit code.
use super::{
    cli::{self, Parsed, UsageError},
    read_dispatch, render,
};
use hf_application::ctl::task_reads::{ArtifactTargetPort, PendingLimits, Reads};
use hf_domain::{
    ctl::{
        identity::{CTL_FORMAT, HostIdentity, PRODUCT_VERSION, PROGRAM, VERSION_INFO_FORMAT},
        inspect::StateInspection,
        state_root::{InvalidStateRoot, StateRoot},
    },
    process::ProcessIdentity,
};
use serde_json::json;
/// Everything with an effect, supplied by the composition root.
pub trait CtlServices {
    fn resolve_state_root(&self) -> Result<StateRoot, InvalidStateRoot>;
    fn host_identity(&self) -> HostIdentity;
    fn inspect(&self, root: &StateRoot) -> StateInspection;
    /// Whether the store-backed reads exist on this platform. Without them those commands
    /// are an invalid choice.
    fn store_reads_supported(&self) -> bool {
        false
    }
    /// The ports of the store-backed reads, with the deadline and markers of this invocation.
    fn reads(&self) -> Option<Reads<'_>> {
        None
    }
    /// The observer of artifact target files; without it `task-artifacts` is an invalid choice.
    fn artifact_targets(&self) -> Option<&dyn ArtifactTargetPort> {
        None
    }
    /// Whether the root task marker (`HARFLOW_ROOT_TASK_ID`) is set to a non-empty value.
    fn root_task_marker(&self) -> bool {
        false
    }
    /// The current instant, as microseconds since the Unix epoch; `None` when the clock is
    /// unavailable.
    fn now_micros(&self) -> Option<i64> {
        None
    }
    /// How long a statistics read may run before it gives up its page.
    fn statistics_budget(&self) -> std::time::Duration {
        hf_application::ctl::task_reads::STATISTICS_BUDGET
    }
    /// A count of rows after which a statistics read counts its budget as spent.
    fn statistics_expire_after_rows(&self) -> Option<usize> {
        None
    }
    /// The bounds of a pending launch read; only a test seam moves them.
    fn pending_limits(&self) -> PendingLimits {
        PendingLimits::default()
    }
    /// The value of `HARFLOW_RETENTION_DAYS`, when set.
    fn retention_days(&self) -> Option<String> {
        None
    }
    /// The qualified immediate parent, or `None` when it cannot be qualified alive.
    fn observer_parent(&self) -> Option<ProcessIdentity> {
        None
    }
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
/// `--json` or a unique prefix of it, before any `--` terminator: the one decision every
/// error document of the invocation follows.
fn json_flagged(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg.len() > 2 && arg.starts_with("--") && "--json".starts_with(arg.as_str()))
}
pub(super) fn invalid_choice(command: &str, json_requested: bool) -> Outcome {
    let message = format!("argument command: invalid choice: '{command}'");
    let error = UsageError {
        program: cli::PROGRAM.to_string(),
        message,
    };
    usage(CTL_FORMAT, json_requested, &error)
}
pub fn dispatch(argv: &[String], services: &dyn CtlServices) -> Outcome {
    if argv.first().map(String::as_str) == Some("version-info") {
        let rest = argv.get(1..).unwrap_or_default();
        let json_requested = json_flagged(rest);
        return match cli::parse_version_info(rest) {
            Ok(parsed) => run(parsed, services, VERSION_INFO_FORMAT, json_requested),
            Err(error) => usage(VERSION_INFO_FORMAT, json_requested, &error),
        };
    }
    let json_requested = json_flagged(argv);
    let help_requested = has_flag(argv, &["--help", "-h"]);
    if !help_requested && let Err(invalid) = services.resolve_state_root() {
        let error = UsageError {
            program: cli::PROGRAM.to_string(),
            message: invalid.message.to_string(),
        };
        return usage(CTL_FORMAT, json_requested, &error);
    }
    match cli::parse_ctl(argv, services.store_reads_supported()) {
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
        read => match services.resolve_state_root() {
            Ok(root) => read_dispatch::run_read(&read, &root, services),
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
        let document = render::version_info(
            &root,
            &services.host_identity(),
            services.store_reads_supported(),
        );
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
