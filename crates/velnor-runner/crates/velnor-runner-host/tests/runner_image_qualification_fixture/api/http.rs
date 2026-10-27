// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const HEADER_LIMIT: usize = 64 * 1024;
const BODY_LIMIT: usize = 1024 * 1024;

pub(super) struct Request {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) body: Vec<u8>,
}

pub(super) struct Reply {
    pub(super) status: u16,
    pub(super) body: Vec<u8>,
    pub(super) delay: Option<Duration>,
    pub(super) disconnect: bool,
    pub(super) upgraded: bool,
}

impl Reply {
    pub(super) fn json(status: u16, body: &serde_json::Value) -> Self {
        Self {
            status,
            body: serde_json::to_vec(body).unwrap_or_default(),
            delay: None,
            disconnect: false,
            upgraded: false,
        }
    }

    pub(super) fn bytes(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            delay: None,
            disconnect: false,
            upgraded: false,
        }
    }

    pub(super) fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    pub(super) fn disconnect(mut self) -> Self {
        self.disconnect = true;
        self
    }

    pub(super) fn upgrade(mut self) -> Self {
        self.upgraded = true;
        self
    }
}

pub(super) fn read_request(stream: &mut UnixStream) -> io::Result<Request> {
    let mut header = Vec::with_capacity(1024);
    let mut byte = [0_u8; 1];
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() == HEADER_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "header too large",
            ));
        }
        stream.read_exact(&mut byte)?;
        header.push(byte[0]);
    }
    let text = std::str::from_utf8(&header)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid header"))?;
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers = HashMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid content length"))?
        .unwrap_or(0);
    if length > BODY_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body)?;
    Ok(Request { method, path, body })
}

pub(super) fn write_reply(stream: &mut UnixStream, reply: &Reply) -> io::Result<()> {
    if reply.upgraded {
        stream
            .write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")?;
        if let Some(delay) = reply.delay {
            std::thread::sleep(delay);
        }
        stream.write_all(&reply.body)?;
        stream.shutdown(std::net::Shutdown::Both)?;
        return Ok(());
    }
    if reply.disconnect {
        return Ok(());
    }
    if let Some(delay) = reply.delay {
        std::thread::sleep(delay);
    }
    let reason = match reply.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Bad Request",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.status,
        reason,
        reply.body.len()
    )?;
    stream.write_all(&reply.body)?;
    stream.shutdown(std::net::Shutdown::Both)
}
