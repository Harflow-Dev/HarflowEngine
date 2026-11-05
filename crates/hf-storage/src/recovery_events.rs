use crate::{Error, Limits, Result, connection};
use hf_domain::{
    capture::{CaptureProgress, CaptureSnapshot},
    ownership::{Digest, Id},
    process::ProcessExit,
};
use rusqlite::{Connection, types::ValueRef};
use sha2::{Digest as _, Sha256};
use std::time::Instant;
pub(crate) fn validate(
    db: &Connection,
    id: &Id,
    capture: Option<&CaptureSnapshot>,
    limits: Limits,
    deadline: Instant,
) -> Result<(u32, Digest)> {
    let mut statement = db.prepare("SELECT CASE WHEN typeof(revision)='integer' THEN revision END,CASE WHEN typeof(stdout)='integer' THEN stdout END,CASE WHEN typeof(stderr)='integer' THEN stderr END,CASE WHEN typeof(stdout_eof)='integer' THEN stdout_eof END,CASE WHEN typeof(stderr_eof)='integer' THEN stderr_eof END,CASE WHEN typeof(loss)='integer' THEN loss END,CASE WHEN typeof(exit_kind)='integer' THEN exit_kind END,CASE WHEN typeof(exit_value)='integer' THEN exit_value END FROM capture_events WHERE operation=?1 ORDER BY revision LIMIT 129")?;
    let mut rows = statement.query([id.as_str()])?;
    let mut count = 0u32;
    let mut hash = Sha256::new();
    let mut prior = CaptureProgress {
        revision: 0,
        stdout: 0,
        stderr: 0,
        stdout_eof: false,
        stderr_eof: false,
        loss: false,
    };
    let mut exit = ProcessExit::Unobserved;
    while let Some(row) = rows.next()? {
        connection::check_deadline(deadline)?;
        count += 1;
        if count > 128
            || count as usize > limits.max_rows
            || count as usize * 104 > limits.max_bytes
        {
            return Err(Error::LimitExceeded);
        }
        let mut fields = [0i64; 8];
        for (index, field) in fields.iter_mut().enumerate() {
            let ValueRef::Integer(value) = row.get_ref(index)? else {
                return Err(Error::InvalidOwnership);
            };
            *field = value;
            hash.update(value.to_be_bytes());
        }
        let (next, next_exit) = decode(fields)?;
        transition(&prior, &next, exit, next_exit)?;
        prior = next;
        exit = next_exit;
    }
    match capture {
        Some(c)
            if c.progress == prior && c.exit == exit && c.progress.revision == u64::from(count) => {
        }
        None if count == 0 => (),
        _ => return Err(Error::InvalidOwnership),
    }
    let digest =
        Digest::new(format!("{:x}", hash.finalize())).map_err(|_| Error::InvalidOwnership)?;
    Ok((count, digest))
}
fn decode(v: [i64; 8]) -> Result<(CaptureProgress, ProcessExit)> {
    if v[0] <= 0
        || !(0..=65536).contains(&v[1])
        || !(0..=65536).contains(&v[2])
        || v[3..6].iter().any(|b| !(0..=1).contains(b))
    {
        return Err(Error::InvalidOwnership);
    }
    let exit = match (v[6], v[7]) {
        (0, 0) => ProcessExit::Unobserved,
        (1, n) => ProcessExit::Code(i32::try_from(n).map_err(|_| Error::InvalidOwnership)?),
        (2, n) if n > 0 => {
            ProcessExit::Signal(i32::try_from(n).map_err(|_| Error::InvalidOwnership)?)
        }
        _ => return Err(Error::InvalidOwnership),
    };
    Ok((
        CaptureProgress {
            revision: v[0] as u64,
            stdout: v[1] as u64,
            stderr: v[2] as u64,
            stdout_eof: v[3] == 1,
            stderr_eof: v[4] == 1,
            loss: v[5] == 1,
        },
        exit,
    ))
}
fn transition(
    p: &CaptureProgress,
    n: &CaptureProgress,
    prior_exit: ProcessExit,
    exit: ProcessExit,
) -> Result<()> {
    let same = p.stdout == n.stdout
        && p.stderr == n.stderr
        && p.stdout_eof == n.stdout_eof
        && p.stderr_eof == n.stderr_eof
        && p.loss == n.loss;
    if n.revision != p.revision + 1
        || n.stdout < p.stdout
        || n.stderr < p.stderr
        || (p.stdout_eof && (!n.stdout_eof || p.stdout != n.stdout))
        || (p.stderr_eof && (!n.stderr_eof || p.stderr != n.stderr))
        || (p.loss && !n.loss)
        || prior_exit != ProcessExit::Unobserved
    {
        return Err(Error::InvalidOwnership);
    }
    if exit == ProcessExit::Unobserved {
        if same {
            return Err(Error::InvalidOwnership);
        }
    } else if !same || !n.stdout_eof || !n.stderr_eof || n.loss {
        return Err(Error::InvalidOwnership);
    }
    Ok(())
}
