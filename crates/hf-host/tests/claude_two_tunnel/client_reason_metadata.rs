use hf_domain::process::ProcessIdentity;
pub type Category = &'static str;
#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    pub call: usize,
    pub outer: Category,
    pub cause: Category,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Observation {
    pub records: Vec<Record>,
    pub calls: usize,
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
        calls: 0,
        overflow: false,
        complete: false,
        healthy: false,
    };
    for (index, line) in text.lines().enumerate() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() < 3 || fields[1] != nonce || fields[2] != pid || result.complete {
            return Err("unavailable");
        }
        match fields[0] {
            "CR1" if index == 0 && fields.len() == 3 => {}
            "CE1" if index > 0 && fields.len() == 6 && result.records.len() < 4 => {
                let call = number(fields[3])?;
                if !(1..=4).contains(&call) || result.records.iter().any(|r| r.call == call) {
                    return Err("unavailable");
                }
                result.records.push(Record {
                    call,
                    outer: category(fields[4])?,
                    cause: category(fields[5])?,
                });
            }
            "CO1" if index > 0 && fields.len() == 3 && !result.overflow => result.overflow = true,
            "CX1" if index > 0 && fields.len() == 7 => finish(&mut result, &fields)?,
            _ => return Err("unavailable"),
        }
    }
    Ok(result)
}
fn finish(result: &mut Observation, fields: &[&str]) -> Result<(), &'static str> {
    let calls = number(fields[3])?;
    let records = number(fields[4])?;
    let overflow = number(fields[5])?;
    let healthy = number(fields[6])?;
    if calls > 4
        || records != result.records.len()
        || records > calls.min(4)
        || overflow > 1
        || healthy > 1
        || (overflow == 1) != result.overflow
        || (result.overflow && calls != 4)
        || result.records.iter().any(|r| r.call > calls)
    {
        return Err("unavailable");
    }
    result.calls = calls;
    result.complete = true;
    result.healthy = healthy == 1;
    Ok(())
}
