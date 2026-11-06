use hf_domain::process::ProcessIdentity;
pub type Category = &'static str;
#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    pub call: usize,
    pub outer: Category,
    pub cause: Category,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Origin {
    Returned,
    Threw,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Terminal {
    Error,
    Fulfilled,
    AttachmentFault,
    ObserverFault,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Call {
    pub call: usize,
    pub origin: Option<Origin>,
    pub terminal: Option<Terminal>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Observation {
    pub records: Vec<Record>,
    pub calls: Vec<Call>,
    pub reported_calls: Option<usize>,
    pub overflow: bool,
    pub complete: bool,
    pub healthy: bool,
}
const CATEGORIES: &[&str] = &[
    "missing",
    "accessor",
    "unknown",
    "trap",
    "verify",
    "issuer",
    "selfsigned",
    "expired",
    "hostname",
    "reset",
    "refused",
    "not_yet_valid",
    "protocol",
    "timeout",
    "cancelled",
    "broken_pipe",
    "not_connected",
];
fn category(value: &str) -> Result<Category, &'static str> {
    CATEGORIES
        .iter()
        .copied()
        .find(|c| *c == value)
        .ok_or("unavailable")
}
fn number(value: &str) -> Result<usize, &'static str> {
    if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || value.len() > 1 && value.starts_with('0')
    {
        return Err("unavailable");
    }
    value.parse().map_err(|_| "unavailable")
}
pub fn parse(
    bytes: &[u8],
    nonce: &str,
    identity: &ProcessIdentity,
) -> Result<Observation, &'static str> {
    if bytes.contains(&b'\r')
        || bytes.len() > 512
        || !bytes.ends_with(b"\n")
        || nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("unavailable");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "unavailable")?;
    let pid = identity.pid().to_string();
    let mut result = Observation {
        records: Vec::new(),
        calls: Vec::new(),
        reported_calls: None,
        overflow: false,
        complete: false,
        healthy: false,
    };
    for (index, line) in text.lines().enumerate() {
        if index >= 15 || result.complete {
            return Err("unavailable");
        }
        let fields: Vec<_> = line.split('\t').collect();
        if index == 0 {
            if fields != ["CR2", nonce, &pid] {
                return Err("unavailable");
            }
        } else if fields.len() == 2 {
            phase(&mut result, fields[0], number(fields[1])?)?;
        } else {
            if fields.len() < 3 || fields[1] != nonce || fields[2] != pid {
                return Err("unavailable");
            }
            match fields[0] {
                "CE2" if fields.len() == 6 => {
                    let call = number(fields[3])?;
                    terminal(&mut result, call, Terminal::Error)?;
                    result.records.push(Record {
                        call,
                        outer: category(fields[4])?,
                        cause: category(fields[5])?,
                    });
                }
                "CO2" if fields.len() == 3 && result.calls.len() == 4 && !result.overflow => {
                    result.overflow = true
                }
                "CX2" if fields.len() == 7 => finish(&mut result, &fields)?,
                _ => return Err("unavailable"),
            }
        }
    }
    Ok(result)
}
fn phase(result: &mut Observation, tag: &str, id: usize) -> Result<(), &'static str> {
    if tag == "CI2" && id == result.calls.len() + 1 && id <= 4 {
        result.calls.push(Call {
            call: id,
            origin: None,
            terminal: None,
        });
        return Ok(());
    }
    if tag == "CN2" || tag == "CS2" {
        let call = result
            .calls
            .get_mut(id.checked_sub(1).ok_or("unavailable")?)
            .ok_or("unavailable")?;
        if call.origin.is_some() {
            return Err("unavailable");
        }
        call.origin = Some(if tag == "CN2" {
            Origin::Returned
        } else {
            Origin::Threw
        });
        return Ok(());
    }
    let value = match tag {
        "CF2" => Terminal::Fulfilled,
        "CA2" => Terminal::AttachmentFault,
        "CU2" => Terminal::ObserverFault,
        _ => return Err("unavailable"),
    };
    terminal(result, id, value)
}
fn terminal(result: &mut Observation, id: usize, value: Terminal) -> Result<(), &'static str> {
    let call = result
        .calls
        .get_mut(id.checked_sub(1).ok_or("unavailable")?)
        .ok_or("unavailable")?;
    if call.origin.is_none()
        || call.terminal.is_some()
        || (call.origin == Some(Origin::Threw)
            && !matches!(value, Terminal::Error | Terminal::ObserverFault))
    {
        return Err("unavailable");
    }
    call.terminal = Some(value);
    Ok(())
}
fn finish(result: &mut Observation, fields: &[&str]) -> Result<(), &'static str> {
    let calls = number(fields[3])?;
    let records = number(fields[4])?;
    let overflow = number(fields[5])?;
    let healthy = number(fields[6])?;
    let fault = result.calls.iter().any(|c| {
        matches!(
            c.terminal,
            Some(Terminal::AttachmentFault | Terminal::ObserverFault)
        )
    });
    if calls != result.calls.len()
        || records != result.records.len()
        || overflow > 1
        || healthy > 1
        || (overflow == 1) != result.overflow
        || (healthy == 1 && fault)
    {
        return Err("unavailable");
    }
    result.reported_calls = Some(calls);
    result.complete = true;
    result.healthy = healthy == 1;
    Ok(())
}
