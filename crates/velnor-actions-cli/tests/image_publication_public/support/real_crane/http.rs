// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
};

pub(super) const MAX_HEADER_BYTES: usize = 64 * 1024;
pub(super) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct Request {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) query: Option<String>,
    pub(super) body: Vec<u8>,
}

pub(super) struct Response {
    status: u16,
    reason: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub(super) fn read_request(reader: &mut BufReader<TcpStream>) -> io::Result<Request> {
    let mut header_bytes = 0;
    let line = read_line(reader, &mut header_bytes)?;
    let mut fields = line.split_ascii_whitespace();
    let method = fields.next().ok_or_else(invalid_request)?.to_owned();
    let target = fields.next().ok_or_else(invalid_request)?;
    let (path, query) = target
        .split_once('?')
        .map_or((target, None), |(path, query)| {
            (path, Some(query.to_owned()))
        });
    let mut headers = BTreeMap::new();
    loop {
        let line = read_line(reader, &mut header_bytes)?;
        if line.trim().is_empty() {
            break;
        }
        let (key, value) = line.split_once(':').ok_or_else(invalid_request)?;
        headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
    if headers
        .get("expect")
        .is_some_and(|value| value.eq_ignore_ascii_case("100-continue"))
    {
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let body = read_body(reader, &headers)?;
    Ok(Request {
        method,
        path: path.to_owned(),
        query,
        body,
    })
}

pub(super) fn write_response(
    stream: &mut TcpStream,
    response: Response,
    head: bool,
) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\nDocker-Distribution-Api-Version: registry/2.0\r\n",
        response.status,
        response.reason,
        response.body.len()
    )?;
    for (key, value) in response.headers {
        write!(stream, "{key}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    if !head {
        stream.write_all(&response.body)?;
    }
    stream.flush()
}

pub(super) fn response(
    status: u16,
    reason: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> Response {
    Response {
        status,
        reason,
        headers,
        body,
    }
}

pub(super) fn json_response(status: u16, reason: &'static str, body: Vec<u8>) -> Response {
    response(
        status,
        reason,
        vec![("Content-Type".to_owned(), "application/json".to_owned())],
        body,
    )
}

pub(super) fn not_found(code: &str, message: &str) -> Response {
    let body =
        format!("{{\"errors\":[{{\"code\":\"{code}\",\"message\":\"{message}\"}}]}}").into_bytes();
    json_response(404, "Not Found", body)
}

pub(super) fn invalid_request() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid registry request")
}

fn read_line(reader: &mut BufReader<TcpStream>, total: &mut usize) -> io::Result<String> {
    let mut bytes = Vec::new();
    loop {
        let (count, newline) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            let count = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            if total.saturating_add(count) > MAX_HEADER_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "registry headers too large",
                ));
            }
            bytes.extend_from_slice(&available[..count]);
            (count, available[count - 1] == b'\n')
        };
        reader.consume(count);
        *total += count;
        if newline {
            return String::from_utf8(bytes).map_err(io::Error::other);
        }
    }
}

fn read_body(
    reader: &mut BufReader<TcpStream>,
    headers: &BTreeMap<String, String>,
) -> io::Result<Vec<u8>> {
    if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        return read_chunked(reader);
    }
    let length = headers
        .get("content-length")
        .map_or(Ok(0), |value| value.parse::<usize>())
        .map_err(io::Error::other)?;
    if length > MAX_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "registry body too large",
        ));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(body)
}

fn read_chunked(reader: &mut BufReader<TcpStream>) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or_default(), 16)
            .map_err(io::Error::other)?;
        if size == 0 {
            while !line.trim().is_empty() {
                line.clear();
                reader.read_line(&mut line)?;
            }
            return Ok(body);
        }
        if body.len().saturating_add(size) > MAX_BODY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "registry body too large",
            ));
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        let mut terminator = [0; 2];
        reader.read_exact(&mut terminator)?;
        if terminator != *b"\r\n" {
            return Err(invalid_request());
        }
    }
}
