//! Closed milestone metadata, joined to the unchanged session codec.
use super::codec;

#[derive(Debug)]
pub enum Sni {
    Unseen,
    Matched,
    Absent,
    Other,
}
#[derive(Debug)]
pub struct Flags {
    pub read_client_hello: bool,
    pub write_server_hello: bool,
    pub write_certificate: bool,
    pub write_server_key_exchange: bool,
    pub write_server_hello_done: bool,
    pub read_client_key_exchange: bool,
    pub read_ccs: bool,
    pub read_finished: bool,
    pub write_ccs: bool,
    pub write_finished: bool,
    pub read_alert: bool,
    pub write_alert: bool,
}
impl Flags {
    fn from_mask(mask: u16) -> Self {
        Self {
            read_client_hello: mask & 1 != 0,
            write_server_hello: mask & 2 != 0,
            write_certificate: mask & 4 != 0,
            write_server_key_exchange: mask & 8 != 0,
            write_server_hello_done: mask & 16 != 0,
            read_client_key_exchange: mask & 32 != 0,
            read_ccs: mask & 64 != 0,
            read_finished: mask & 128 != 0,
            write_ccs: mask & 256 != 0,
            write_finished: mask & 512 != 0,
            read_alert: mask & 1024 != 0,
            write_alert: mask & 2048 != 0,
        }
    }
}
#[derive(Debug)]
pub struct Milestone {
    pub ordinal: usize,
    pub peer_port: u16,
    pub flags: Flags,
    pub sni: Sni,
}
#[derive(Debug)]
pub struct DiagnosticVerdict {
    pub verdict: codec::Verdict,
    pub milestones: Vec<Milestone>,
}
fn number<T: std::str::FromStr>(value: &str) -> Result<T, &'static str> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid milestone number");
    }
    value.parse().map_err(|_| "invalid milestone number")
}
fn record(line: &str) -> Result<Milestone, &'static str> {
    let fields: Vec<_> = line.split('\t').collect();
    let ["TLS_MILESTONE_V1", ordinal, "127.0.0.1", port, mask, sni] = fields.as_slice() else {
        return Err("invalid milestone record");
    };
    let mask = number::<u16>(mask)?;
    if mask & !4095 != 0 {
        return Err("unknown milestone flags");
    }
    Ok(Milestone {
        ordinal: number(ordinal)?,
        peer_port: number(port)?,
        flags: Flags::from_mask(mask),
        sni: match *sni {
            "unseen" => Sni::Unseen,
            "matched" => Sni::Matched,
            "absent" => Sni::Absent,
            "other" => Sni::Other,
            _ => return Err("invalid milestone SNI"),
        },
    })
}
pub fn parse(raw: &[u8]) -> Result<DiagnosticVerdict, &'static str> {
    if raw.len() > 4096 || !raw.ends_with(b"\n") {
        return Err("invalid milestone metadata bound");
    }
    let text = std::str::from_utf8(raw).map_err(|_| "invalid milestone encoding")?;
    let mut common = String::new();
    let mut done = false;
    let mut milestones: Vec<Milestone> = Vec::new();
    for line in text.lines() {
        if !done {
            common.push_str(line);
            common.push('\n');
            done = line.starts_with("DONE\t");
        } else {
            let row = record(line)?;
            if milestones.len() == 4
                || row.ordinal != milestones.len() + 1
                || row.peer_port == 0
                || milestones
                    .iter()
                    .any(|other| other.peer_port == row.peer_port)
            {
                return Err("inconsistent milestone identity");
            }
            milestones.push(row);
        }
    }
    let verdict = codec::parse_wave(common.as_bytes())?;
    if milestones.len() != verdict.sessions.len()
        || !milestones.iter().all(|row| {
            verdict
                .sessions
                .iter()
                .any(|session| session.ordinal == row.ordinal && session.peer_port == row.peer_port)
        })
    {
        return Err("incomplete milestone coverage");
    }
    Ok(DiagnosticVerdict {
        verdict,
        milestones,
    })
}
