#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_interfaces::backend_measurement::MAX_BODY;
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::Instant,
};

const HEADER_LIMIT: usize = 8192;

pub(super) struct Request {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) headers: std::collections::BTreeMap<String, String>,
    pub(super) body: Vec<u8>,
}
pub(super) fn read_request(stream: &mut TcpStream, end: Instant) -> Result<Request, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 2048];
    let header_end = loop {
        if let Some(index) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() >= HEADER_LIMIT || Instant::now() >= end {
            return Err("HTTP header budget/deadline".into());
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err("HTTP peer closed before headers".into()),
            Ok(count) if bytes.len() + count <= HEADER_LIMIT => {
                bytes.extend_from_slice(&chunk[..count])
            }
            Ok(_) => return Err("HTTP header overflow".into()),
            Err(error) if temporary(&error) => (),
            Err(error) => return Err(show(error)),
        }
    };
    let header_text = std::str::from_utf8(&bytes[..header_end]).map_err(show)?;
    let mut lines = header_text[..header_text.len() - 4].split("\r\n");
    let mut first = lines
        .next()
        .ok_or("missing request line")?
        .split_ascii_whitespace();
    let method = first.next().ok_or("missing method")?.to_owned();
    let path = first.next().ok_or("missing route")?.to_owned();
    if first.next() != Some("HTTP/1.1") || first.next().is_some() {
        return Err("invalid HTTP request line".into());
    }
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        let (key, value) = line.split_once(':').ok_or("invalid HTTP header")?;
        let key = key.to_ascii_lowercase();
        if key == "transfer-encoding" || headers.insert(key, value.trim().to_owned()).is_some() {
            return Err("chunked or duplicate HTTP header refused".into());
        }
    }
    let length = headers
        .get("content-length")
        .map(|value| {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("invalid Content-Length".into());
            }
            value.parse::<usize>().map_err(show)
        })
        .transpose()?
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err("HTTP request body exceeds fixture budget".into());
    }
    if bytes.len() - header_end > length {
        return Err("pipelined or trailing HTTP bytes refused".into());
    }
    bytes.drain(..header_end);
    while bytes.len() < length {
        if Instant::now() >= end {
            return Err("HTTP body deadline".into());
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err("HTTP peer closed before body complete".into()),
            Ok(count) if bytes.len() + count <= length => bytes.extend_from_slice(&chunk[..count]),
            Ok(_) => return Err("HTTP body overflow".into()),
            Err(error) if temporary(&error) => (),
            Err(error) => return Err(show(error)),
        }
    }
    Ok(Request {
        method,
        path,
        headers,
        body: bytes,
    })
}

pub(super) fn reply(stream: &mut TcpStream, body: &[u8]) -> Result<(), String> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/event-stream\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(body))
        .map_err(show)
}
fn temporary(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}
fn show(error: impl std::fmt::Display) -> String {
    error.to_string()
}
