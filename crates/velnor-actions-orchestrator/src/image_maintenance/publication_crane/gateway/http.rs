// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    io::{self, BufReader, Read, Write},
    net::TcpStream,
};

use velnor_actions_mise::CheckDeadline;

const MAX_HEADER_BYTES: usize = 32 * 1024;
const MAX_HEADERS: usize = 64;
const MAX_CHUNK_LINE: usize = 1024;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP framing")
}

fn timeout(deadline: CheckDeadline) -> io::Result<std::time::Duration> {
    deadline
        .remaining()
        .map(|value| value.min(std::time::Duration::from_secs(30)))
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "HTTP deadline"))
}

enum Framing {
    Fixed(u64),
    Chunked {
        remaining: u64,
        next: bool,
        done: bool,
    },
}

pub(super) struct Request {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) headers: Vec<(String, String)>,
    stream: BufReader<TcpStream>,
    framing: Framing,
    remaining_limit: u64,
    deadline: CheckDeadline,
}

impl Request {
    pub(super) fn read(
        stream: TcpStream,
        deadline: CheckDeadline,
        max_body: u64,
    ) -> io::Result<Self> {
        let mut stream = BufReader::new(stream);
        let mut bytes = Vec::new();
        loop {
            let mut byte = [0];
            stream
                .get_ref()
                .set_read_timeout(Some(timeout(deadline)?))?;
            stream.read_exact(&mut byte)?;
            bytes.push(byte[0]);
            if bytes.len() == 1 && !byte[0].is_ascii_uppercase() {
                return Err(invalid());
            }
            if bytes.ends_with(b"\r\n\r\n") {
                break;
            }
            if bytes.len() >= MAX_HEADER_BYTES {
                return Err(invalid());
            }
        }
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut parsed = httparse::Request::new(&mut headers);
        if !matches!(parsed.parse(&bytes), Ok(httparse::Status::Complete(size)) if size == bytes.len())
            || parsed.version != Some(1)
        {
            return Err(invalid());
        }
        let method = parsed.method.ok_or_else(invalid)?.to_owned();
        let target = parsed.path.ok_or_else(invalid)?.to_owned();
        if !target.starts_with('/') || target.starts_with("//") || target.contains('#') {
            return Err(invalid());
        }
        let mut result_headers = Vec::new();
        let mut length = None;
        let mut chunked = false;
        for header in parsed.headers.iter() {
            let value = std::str::from_utf8(header.value).map_err(|_| invalid())?;
            if header.name.eq_ignore_ascii_case("content-length") {
                if length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(invalid());
                }
                length = Some(value.parse::<u64>().map_err(|_| invalid())?);
            }
            if header.name.eq_ignore_ascii_case("transfer-encoding") {
                if chunked || !value.eq_ignore_ascii_case("chunked") {
                    return Err(invalid());
                }
                chunked = true;
            }
            result_headers.push((header.name.to_ascii_lowercase(), value.to_owned()));
        }
        if chunked && length.is_some() || length.is_some_and(|size| size > max_body) {
            return Err(invalid());
        }
        let framing = if chunked {
            Framing::Chunked {
                remaining: 0,
                next: false,
                done: false,
            }
        } else {
            Framing::Fixed(length.unwrap_or(0))
        };
        Ok(Self {
            method,
            target,
            headers: result_headers,
            stream,
            framing,
            remaining_limit: max_body,
            deadline,
        })
    }

    fn read_byte(&mut self) -> io::Result<u8> {
        self.stream
            .get_ref()
            .set_read_timeout(Some(timeout(self.deadline)?))?;
        let mut byte = [0];
        self.stream.read_exact(&mut byte)?;
        Ok(byte[0])
    }

    fn chunk_line(&mut self) -> io::Result<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            line.push(self.read_byte()?);
            if line.ends_with(b"\r\n") {
                line.truncate(line.len() - 2);
                return Ok(line);
            }
            if line.len() >= MAX_CHUNK_LINE {
                return Err(invalid());
            }
        }
    }

    pub(super) fn respond(
        &mut self,
        status: u16,
        headers: &[(String, String)],
        body: &mut dyn Read,
    ) -> io::Result<()> {
        if !(100..=599).contains(&status) {
            return Err(invalid());
        }
        let mut head = format!("HTTP/1.1 {status} Response\r\nConnection: close\r\n");
        for (name, value) in headers {
            if name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || value.bytes().any(|b| b < 32 || b == 127)
                || name.eq_ignore_ascii_case("connection")
                || name.eq_ignore_ascii_case("transfer-encoding")
            {
                return Err(invalid());
            }
            head.push_str(name);
            head.push_str(": ");
            head.push_str(value);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        self.write_all(head.as_bytes())?;
        let mut buffer = vec![0; 32 * 1024].into_boxed_slice();
        loop {
            timeout(self.deadline)?;
            let count = body.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            self.write_all(&buffer[..count])?;
        }
        Ok(())
    }

    fn write_all(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            self.stream
                .get_ref()
                .set_write_timeout(Some(timeout(self.deadline)?))?;
            let count = self.stream.get_mut().write(bytes)?;
            if count == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            bytes = &bytes[count..];
        }
        Ok(())
    }
}

impl Read for Request {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let (mut remaining, chunked, next, done) = match self.framing {
            Framing::Fixed(remaining) => (remaining, false, false, false),
            Framing::Chunked {
                remaining,
                next,
                done,
            } => (remaining, true, next, done),
        };
        if done || !chunked && remaining == 0 {
            return Ok(0);
        }
        if chunked && remaining == 0 {
            if next && (self.read_byte()? != b'\r' || self.read_byte()? != b'\n') {
                return Err(invalid());
            }
            let line = self.chunk_line()?;
            if line.is_empty() || !line.iter().all(u8::is_ascii_hexdigit) {
                return Err(invalid());
            }
            let text = std::str::from_utf8(&line).map_err(|_| invalid())?;
            remaining = u64::from_str_radix(text, 16).map_err(|_| invalid())?;
            if remaining > self.remaining_limit {
                return Err(invalid());
            }
            if remaining == 0 {
                if !self.chunk_line()?.is_empty() {
                    return Err(invalid());
                }
                self.framing = Framing::Chunked {
                    remaining: 0,
                    next: false,
                    done: true,
                };
                return Ok(0);
            }
        }
        let size = usize::try_from(remaining.min(buffer.len() as u64)).map_err(|_| invalid())?;
        self.stream
            .get_ref()
            .set_read_timeout(Some(timeout(self.deadline)?))?;
        let count = self.stream.read(&mut buffer[..size])?;
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        remaining -= count as u64;
        self.remaining_limit -= count as u64;
        self.framing = if chunked {
            Framing::Chunked {
                remaining,
                next: true,
                done: false,
            }
        } else {
            Framing::Fixed(remaining)
        };
        Ok(count)
    }
}
