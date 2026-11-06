//! Strict capture-free helper metadata codec. Errors never contain input bytes.
use std::net::SocketAddr;
#[path = "codec_categories.rs"]
mod categories;
pub use categories::{Category, Phase};
#[derive(Debug)]
pub struct Session {
    pub ordinal: usize,
    pub peer_port: u16,
    pub certificate_rejected: bool,
    pub application_data: bool,
    pub status: &'static str,
    pub closed: bool,
    pub phase: Phase,
    pub category: Category,
}
#[derive(Debug)]
pub struct Verdict {
    pub port: u16,
    pub sessions: Vec<Session>,
}
fn number<T: std::str::FromStr>(value: &str) -> Result<T, &'static str> {
    value.parse().map_err(|_| "invalid helper number")
}
fn boolean(value: &str) -> Result<bool, &'static str> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err("invalid helper boolean"),
    }
}
pub fn readiness(raw: &[u8]) -> Result<u16, &'static str> {
    let text = std::str::from_utf8(raw).map_err(|_| "invalid helper encoding")?;
    let line = text.lines().next().ok_or("missing helper readiness")?;
    let port = number(
        line.strip_prefix("READY\t")
            .ok_or("invalid helper readiness")?,
    )?;
    if port == 0 {
        return Err("invalid helper port");
    }
    Ok(port)
}
pub fn parse(raw: &[u8]) -> Result<Verdict, &'static str> {
    if raw.len() > 4096 || !raw.ends_with(b"\n") {
        return Err("invalid helper metadata bound");
    }
    let port = readiness(raw)?;
    let text = std::str::from_utf8(raw).map_err(|_| "invalid helper encoding")?;
    let mut sessions = Vec::new();
    let mut done = None;
    for line in text.lines().skip(1) {
        if done.is_some() {
            return Err("metadata after helper completion");
        }
        let fields: Vec<_> = line.split('\t').collect();
        match fields.as_slice() {
            [
                "SESSION_V2",
                ordinal,
                "127.0.0.1",
                peer,
                certificate,
                application,
                status,
                closed,
                phase,
                category,
            ] => {
                let status = match *status {
                    "cert_alert" => "cert_alert",
                    "application_data" => "application_data",
                    "eof" => "eof",
                    "timeout" => "timeout",
                    "tls_error" => "tls_error",
                    "io_error" => "io_error",
                    _ => return Err("invalid helper status"),
                };
                let session = Session {
                    ordinal: number(ordinal)?,
                    peer_port: number(peer)?,
                    certificate_rejected: boolean(certificate)?,
                    application_data: boolean(application)?,
                    status,
                    closed: boolean(closed)?,
                    phase: Phase::parse(phase)?,
                    category: Category::parse(category)?,
                };
                if sessions.len() == 2
                    || session.ordinal == 0
                    || session.ordinal > 2
                    || session.peer_port == 0
                    || sessions.iter().any(|s: &Session| {
                        s.ordinal == session.ordinal || s.peer_port == session.peer_port
                    })
                    || !session.closed
                    || session.category.status() != status
                    || !session.category.permits_phase(session.phase)
                    || session.certificate_rejected != (status == "cert_alert")
                    || session.application_data != (status == "application_data")
                {
                    return Err("inconsistent helper session");
                }
                sessions.push(session);
            }
            ["DONE", count] => done = Some(number::<usize>(count)?),
            _ => return Err("invalid helper record"),
        }
    }
    if done != Some(sessions.len())
        || !(1..=sessions.len()).all(|ordinal| sessions.iter().any(|s| s.ordinal == ordinal))
    {
        return Err("incomplete helper coverage");
    }
    Ok(Verdict { port, sessions })
}
pub fn bound_rejection(verdict: &Verdict, upstreams: &[(usize, SocketAddr)]) -> bool {
    !upstreams.is_empty()
        && upstreams.len() <= 2
        && verdict.sessions.len() == upstreams.len()
        && upstreams
            .iter()
            .enumerate()
            .all(|(index, (ordinal, addr))| {
                *ordinal > 0
                    && *ordinal <= upstreams.len()
                    && addr.ip() == std::net::Ipv4Addr::LOCALHOST
                    && !upstreams[..index]
                        .iter()
                        .any(|(id, other)| id == ordinal || other == addr)
                    && verdict
                        .sessions
                        .iter()
                        .filter(|s| s.peer_port == addr.port())
                        .count()
                        == 1
            })
        && verdict.sessions.iter().all(|s| {
            s.certificate_rejected
                && !s.application_data
                && s.closed
                && s.status == "cert_alert"
                && s.category.status() == s.status
                && s.category.permits_phase(s.phase)
        })
}
