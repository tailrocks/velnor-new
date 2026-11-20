// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use zeroize::Zeroize;

use crate::TransportFail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Method {
    Get,

    Post,

    Delete,

    Patch,
}

#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct SessionRequest {
    pub method: Method,

    pub path: String,

    pub query: Option<String>,

    pub headers: Vec<(String, String)>,

    pub body: Vec<u8>,
}

impl fmt::Debug for SessionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("query", &self.query)
            .field("headers", &redacted_headers(&self.headers))
            .field("body", &"[redacted]")
            .finish()
    }
}

fn redacted_headers(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(key, value)| {
            if key.eq_ignore_ascii_case("authorization") {
                (key.as_str(), "[redacted]")
            } else {
                (key.as_str(), value.as_str())
            }
        })
        .collect()
}

impl Drop for SessionRequest {
    fn drop(&mut self) {
        self.body.zeroize();
        for (key, value) in &mut self.headers {
            if key.eq_ignore_ascii_case("authorization") {
                value.zeroize();
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
#[must_use]
pub struct Exchange {
    pub status: u16,

    pub body: Vec<u8>,
}

impl fmt::Debug for Exchange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Exchange")
            .field("status", &self.status)
            .field("body", &"[redacted]")
            .finish()
    }
}

impl Drop for Exchange {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

pub trait Transport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail>;
}
