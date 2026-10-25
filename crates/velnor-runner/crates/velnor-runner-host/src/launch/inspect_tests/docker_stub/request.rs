//! Read complete Docker test requests within the header and body limits.

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RequestTrace {
    pub(super) path: Option<String>,
    pub(super) response_index: Option<usize>,
    pub(super) response_status: Option<u16>,
    pub(super) failure: Option<String>,
}

pub(super) struct RequestFailure {
    pub(super) path: Option<String>,
    pub(super) message: String,
}

pub(super) fn request_path(line: &str) -> Result<String, String> {
    line.split_ascii_whitespace()
        .nth(1)
        .map(|target| target.split('?').next().unwrap_or(target).to_owned())
        .ok_or_else(|| "The Docker test request target is missing.".to_owned())
}

#[derive(Default)]
pub(super) struct RequestFraming {
    head: Vec<u8>,
    line: Option<String>,
    body_remaining: Option<usize>,
}

impl RequestFraming {
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<bool, String> {
        let Some(remaining) = self.body_remaining.as_mut() else {
            return self.push_head(bytes);
        };
        if bytes.len() > *remaining {
            return Err("The Docker test request has bytes beyond its declared body.".to_owned());
        }
        *remaining -= bytes.len();
        Ok(*remaining == 0)
    }

    fn push_head(&mut self, bytes: &[u8]) -> Result<bool, String> {
        self.head.extend_from_slice(bytes);
        let Some(end) = header_end(&self.head) else {
            if self.head.len() > MAX_HEADER_BYTES {
                return Err("The Docker test request headers exceed 64 KiB.".to_owned());
            }
            return Ok(false);
        };
        if end
            .checked_add(4)
            .is_none_or(|header_bytes| header_bytes > MAX_HEADER_BYTES)
        {
            return Err("The Docker test request headers exceed 64 KiB.".to_owned());
        }
        let (line, body_length) = parse_head(&self.head[..end])?;
        let body_start = end + 4;
        let body_bytes = self.head.len() - body_start;
        if body_bytes > body_length {
            return Err("The Docker test request exceeds its declared body length.".to_owned());
        }
        self.line = Some(line);
        self.body_remaining = Some(body_length - body_bytes);
        self.head.clear();
        Ok(body_bytes == body_length)
    }

    pub(super) fn request_line(&self) -> Option<&str> {
        self.line.as_deref()
    }

    pub(super) fn diagnostic_path(&self) -> Option<String> {
        let line = self
            .line
            .as_deref()
            .or_else(|| std::str::from_utf8(&self.head).ok()?.split("\r\n").next())?;
        request_path(line).ok()
    }

    pub(super) fn failure(&self, message: impl Into<String>) -> RequestFailure {
        RequestFailure {
            path: self.diagnostic_path(),
            message: message.into(),
        }
    }

    pub(super) fn finish(&self) -> Result<&str, String> {
        if self.body_remaining == Some(0) {
            return self
                .request_line()
                .ok_or_else(|| "The Docker test request line is missing.".to_owned());
        }
        Err("The Docker test request ended before its headers or body were complete.".to_owned())
    }
}

fn header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_head(bytes: &[u8]) -> Result<(String, usize), String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "The Docker test request headers are not valid UTF-8.".to_owned())?;
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| "The Docker test request line is missing.".to_owned())?;
    validate_request_line(request_line)?;
    let mut content_length = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "The Docker test request header is invalid.".to_owned())?;
        if name.is_empty() || !name.bytes().all(is_header_name_byte) {
            return Err("The Docker test request header name is invalid.".to_owned());
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("The Docker test server does not support Transfer-Encoding.".to_owned());
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err("The Docker test request Content-Length value is invalid.".to_owned());
            }
            let parsed = value.parse::<usize>().map_err(|_| {
                "The Docker test request Content-Length value is invalid.".to_owned()
            })?;
            if content_length.is_some_and(|known| known != parsed) {
                return Err(
                    "The Docker test request has conflicting Content-Length headers.".to_owned(),
                );
            }
            content_length = Some(parsed);
        }
    }
    let body_length = content_length.unwrap_or(0);
    if body_length > MAX_BODY_BYTES {
        return Err("The Docker test request body exceeds 32 MiB.".to_owned());
    }
    Ok((request_line.to_owned(), body_length))
}

fn validate_request_line(line: &str) -> Result<(), String> {
    let mut parts = line.split_ascii_whitespace();
    let method = parts.next();
    let target = parts.next();
    let version = parts.next();
    if method.is_none_or(str::is_empty)
        || target.is_none_or(str::is_empty)
        || !matches!(version, Some("HTTP/1.0" | "HTTP/1.1"))
        || parts.next().is_some()
    {
        return Err("The Docker test request line is invalid.".to_owned());
    }
    Ok(())
}

fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

#[cfg(test)]
#[path = "request/tests.rs"]
mod tests;
