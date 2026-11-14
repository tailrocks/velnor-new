// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fmt::Write as _, path::Path, time::Duration};

use velnor_runner_github::{Method, SessionRequest};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurlConfigError;

impl std::fmt::Display for CurlConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid curl config input")
    }
}

impl std::error::Error for CurlConfigError {}

pub struct CurlConfig(Zeroizing<String>);

impl std::fmt::Debug for CurlConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CurlConfig([REDACTED])")
    }
}

impl CurlConfig {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn encode(
    url: &str,
    request: &SessionRequest,
    body: &Path,
    max_time: Duration,
) -> Result<CurlConfig, CurlConfigError> {
    if max_time.is_zero() {
        return Err(CurlConfigError);
    }
    let millis = max_time.as_nanos().div_ceil(1_000_000);
    let body_path = if matches!(request.method, Method::Post | Method::Patch) {
        if body.as_os_str().is_empty() || body == Path::new("-") {
            return Err(CurlConfigError);
        }
        Some(body.to_str().ok_or(CurlConfigError)?)
    } else {
        None
    };
    let input_bytes = request
        .headers
        .iter()
        .try_fold(url.len(), |sum, (name, value)| {
            sum.checked_add(name.len())?
                .checked_add(value.len())?
                .checked_add(32)
        })
        .and_then(|sum| sum.checked_add(body_path.map_or(0, str::len)))
        .and_then(|sum| sum.checked_mul(2))
        .and_then(|sum| sum.checked_add(512))
        .ok_or(CurlConfigError)?;
    let mut text = Zeroizing::new(String::new());
    text.try_reserve_exact(input_bytes)
        .map_err(|_| CurlConfigError)?;
    text.push_str("request = \"");
    text.push_str(crate::https::method_name(request.method));
    text.push_str("\"\n");
    quoted(&mut text, "url", &[url])?;
    text.push_str("\noutput = \"-\"\nwrite-out = \"%{http_code}%{stderr}%header{retry-after}\"\n");
    write!(text, "max-time = {}.{:03}", millis / 1000, millis % 1000)
        .map_err(|_| CurlConfigError)?;
    for (name, value) in &request.headers {
        text.push('\n');
        quoted(&mut text, "header", &[name, ": ", value])?;
    }
    if let Some(path) = body_path {
        text.push('\n');
        quoted(&mut text, "data-binary", &["@", path])?;
    }
    Ok(CurlConfig(text))
}

fn quoted(output: &mut String, key: &str, parts: &[&str]) -> Result<(), CurlConfigError> {
    if parts.iter().any(|value| {
        value
            .chars()
            .any(|ch| matches!(ch, '"' | '\n' | '\r' | '\0'))
    }) {
        return Err(CurlConfigError);
    }
    output.push_str(key);
    output.push_str(" = \"");
    for ch in parts.iter().flat_map(|value| value.chars()) {
        if ch == '\\' {
            output.push('\\');
        }
        output.push(ch);
    }
    output.push('"');
    Ok(())
}
