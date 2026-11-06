use crate::process::{
    MacIdentity, OwnedSignal, RetainedOwnedChild, SpawnInitialization, SpawnSpec,
};
use hf_domain::process::ProcessExit;
use std::{
    ffi::OsString,
    io,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
fn refused() -> io::Error {
    io::Error::other("credential broker refused or unsettled")
}
/// Reads only the current OS account's exact service. No store mutation or refresh.
/// OS spawn/probes are nonpreemptible; checks bookend them. Failure retains the
/// active original waiter until positive exit and both physical pipe EOFs.
/// Cleanup may remain resident indefinitely; no hard teardown bound is claimed.
pub fn read_credential_record(until: Instant) -> io::Result<Vec<u8>> {
    let until = until.min(Instant::now() + Duration::from_secs(8));
    let user = run("/usr/bin/id", vec!["-un".into()], until)?;
    let user = std::str::from_utf8(&user)
        .map_err(|_| refused())?
        .trim_end_matches('\n');
    if user.is_empty()
        || user.len() > 256
        || !user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(refused());
    }
    run(
        "/usr/bin/security",
        vec![
            "find-generic-password".into(),
            "-s".into(),
            "Claude Code-credentials".into(),
            "-a".into(),
            user.into(),
            "-w".into(),
        ],
        until,
    )
}
fn run(executable: &str, args: Vec<OsString>, until: Instant) -> io::Result<Vec<u8>> {
    if Instant::now() >= until {
        return Err(refused());
    }
    let spec = SpawnSpec {
        executable: PathBuf::from(executable),
        cwd: std::env::current_dir().map_err(|_| refused())?,
        args,
        environment: Vec::new(),
        capture_limit: 65536,
    };
    let mut child = RetainedOwnedChild::spawn(spec, &MacIdentity, until).map_err(|_| refused())?;
    child.close_stdin();
    let result = if child.initialization() == SpawnInitialization::Ready {
        collect(&mut child, until)
    } else {
        Err(refused())
    };
    if result.is_err() {
        retain_until_settled(&mut child);
        return Err(refused());
    }
    let (output, _) = child.take_capture_bytes();
    Ok(output)
}
fn collect(child: &mut RetainedOwnedChild, until: Instant) -> io::Result<()> {
    while Instant::now() < until {
        child.pump(until, 65536).map_err(|_| refused())?;
        let exit = child.try_wait().map_err(|_| refused())?;
        let capture = child.capture();
        if capture.stdout.bytes.len() + capture.stderr.bytes.len() > 65536
            || capture.stdout.overflow
            || capture.stderr.overflow
            || capture.stdout.error.is_some()
            || capture.stderr.error.is_some()
        {
            return Err(refused());
        }
        if exit != ProcessExit::Unobserved && capture.stdout.eof && capture.stderr.eof {
            return if exit == ProcessExit::Code(0) && capture.stderr.bytes.is_empty() {
                Ok(())
            } else {
                Err(refused())
            };
        }
    }
    Err(refused())
}

/// Never return an unsettled original child to a caller that could exit custody.
/// Persistent pipe/wait failures retain the handle here with a fixed observation
/// backoff, no logging and no root cleanup authority.
fn retain_until_settled(child: &mut RetainedOwnedChild) {
    let _ = child.try_wait();
    if child.exit() == ProcessExit::Unobserved
        && let Some(identity) = child.registered_identity().cloned()
    {
        let _ = child.request_signal(
            &identity,
            &MacIdentity,
            OwnedSignal::Kill,
            Instant::now() + Duration::from_secs(2),
        );
    }
    loop {
        let _ = child.pump(Instant::now() + Duration::from_millis(20), 65536);
        let _ = child.try_wait();
        if child.exit() != ProcessExit::Unobserved
            && child.capture().stdout.eof
            && child.capture().stderr.eof
        {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
}
