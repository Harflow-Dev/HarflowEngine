//! Recovery argument parsing shares the ordinary argparse-compatible scanner.
use super::{
    cli::{Parsed, UsageError},
    cli_scan::{Opt, Scan, error},
};
pub(super) const OPTIONS: [Opt; 2] = [
    Opt {
        flag: "--operation-id",
        value: true,
    },
    Opt {
        flag: "--confirmation-token",
        value: true,
    },
];
pub(super) fn parse(program: &str, parsed: &Scan) -> Result<Parsed, UsageError> {
    let missing: Vec<_> = OPTIONS
        .iter()
        .filter(|option| parsed.value(option.flag).is_none())
        .map(|option| option.flag)
        .collect();
    if !missing.is_empty() {
        return Err(error(
            program,
            format!(
                "the following arguments are required: {}",
                missing.join(", ")
            ),
        ));
    }
    Ok(Parsed::StateRecover {
        json: parsed.json,
        operation_id: parsed
            .value("--operation-id")
            .unwrap_or_default()
            .to_string(),
        confirmation_token: parsed
            .value("--confirmation-token")
            .unwrap_or_default()
            .to_string(),
    })
}
