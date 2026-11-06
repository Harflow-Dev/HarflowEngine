//! Private fixed-helper composition. No public execution or permission admission.
mod actor;
mod capture;
mod launch;
mod owner;
mod retention;
mod writer;
pub(super) fn run(mode: &str) -> Result<(), String> {
    match mode {
        "--internal-storage-writer-fixture-v1" => writer::run(false),
        "--internal-isolated-owner-fixture-v1" => owner::run(false),
        hf_domain::claude_qualification::OWNER_MODE => owner::run(true),
        hf_domain::claude_qualification::WRITER_MODE => writer::run(true),
        _ => Err("private owner runtime mode refused".into()),
    }
}

#[cfg(test)]
mod tests;
