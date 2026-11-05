//! Private fixed-helper composition. No public execution or permission admission.
mod actor;
mod capture;
mod owner;
mod retention;
mod writer;
pub(super) fn run(mode: &str) -> Result<(), String> {
    match mode {
        "--internal-storage-writer-fixture-v1" => writer::run(),
        "--internal-isolated-owner-fixture-v1" => owner::run(),
        _ => Err("private owner runtime mode refused".into()),
    }
}

#[cfg(test)]
mod tests;
