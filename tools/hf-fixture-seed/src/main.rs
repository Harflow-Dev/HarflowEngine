//! Private conformance fixture adapter for Rust fixture roots. Invoked as
//! `hf-fixture-seed OP --root ROOT` with one JSON object on stdin; writes one JSON value
//! on stdout and exits 0, or exits nonzero with a message on stderr. It assumes no
//! `HOME`, `TMPDIR` or locale, and is not reachable from the public engine binary.
mod artifacts;
mod digest;
mod events;
mod fail;
mod identity;
mod launch;
mod ops;
mod root;
mod tasks;
use std::{io::Read, io::Write, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(output) => {
            let mut stdout = std::io::stdout();
            if stdout.write_all(output.as_bytes()).is_err() || stdout.flush().is_err() {
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "hf-fixture-seed: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<String, fail::Fail> {
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let usage = || fail::Fail::new("usage: hf-fixture-seed OP --root ROOT");
    let [operation, flag, root] = arguments.as_slice() else {
        return Err(usage());
    };
    let operation = operation.to_str().ok_or_else(usage)?;
    if flag != "--root" {
        return Err(usage());
    }
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return Err(fail::Fail::new("the root must be an absolute path"));
    }
    let limit = hf_interfaces::fixture_port::MAX_REQUEST as u64;
    let mut stdin = Vec::new();
    std::io::stdin().take(limit + 1).read_to_end(&mut stdin)?;
    let request = hf_interfaces::fixture_port::parse(operation, &stdin)?;
    let response = ops::execute(&root::Context::new(root), request)?;
    Ok(format!(
        "{}\n",
        hf_interfaces::fixture_port::render(&response)
    ))
}
