// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::time::Duration;

use ureq::http::Request as HttpRequest;
use ureq::http::request::Parts;
use ureq::{AsSendBody, SendBody};

use crate::error::CiError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const SEND_TIMEOUT: Duration = Duration::from_mins(30);
const RESPONSE_TIMEOUT: Duration = Duration::from_mins(5);
const RECEIVE_TIMEOUT: Duration = Duration::from_mins(30);

pub enum Body<'a> {
    Empty,
    Bytes(&'a [u8]),
    Stream {
        reader: Box<dyn Read + Send + 'a>,
        length: u64,
    },
}

impl fmt::Debug for Body<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("Empty"),
            Self::Bytes(bytes) => write!(formatter, "Bytes({})", bytes.len()),
            Self::Stream { length, .. } => write!(formatter, "Stream({length})"),
        }
    }
}

pub struct Request<'a> {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Body<'a>,
}

impl fmt::Debug for Request<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Request")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("body", &self.body)
            .finish_non_exhaustive()
    }
}

pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Box<dyn Read + Send>,
}

impl fmt::Debug for Response {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Response")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Response {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    pub fn bounded(mut self, limit: u64) -> Result<Vec<u8>, CiError> {
        let mut data = Vec::new();
        self.body
            .by_ref()
            .take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|_| CiError::new("Registry response could not be read."))?;
        if data.len() as u64 > limit {
            return Err(CiError::new("Registry response exceeds its size limit."));
        }
        Ok(data)
    }
}

pub trait Transport: Sync {
    fn send(&self, request: Request<'_>) -> Result<Response, CiError>;
}

#[derive(Debug)]
pub struct Https {
    agent: ureq::Agent,
}

impl Default for Https {
    fn default() -> Self {
        Self::new()
    }
}

impl Https {
    #[must_use]
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_send_body(Some(SEND_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(RECEIVE_TIMEOUT))
            .user_agent("velnor-ci")
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(crate::trusted_roots::configured_roots())
                    .build(),
            )
            .build()
            .into();
        Self { agent }
    }

    fn run(&self, parts: Parts, body: SendBody<'_>) -> Result<Response, CiError> {
        let response = self
            .agent
            .run(HttpRequest::from_parts(parts, body))
            .map_err(|error| {
                CiError::new(format!("Registry request failed or timed out: {error}"))
            })?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                Some((
                    name.as_str().to_ascii_lowercase(),
                    value.to_str().ok()?.to_owned(),
                ))
            })
            .collect();
        Ok(Response {
            status,
            headers,
            body: Box::new(response.into_body().into_reader()),
        })
    }
}

impl Transport for Https {
    fn send(&self, request: Request<'_>) -> Result<Response, CiError> {
        let Request {
            method,
            url,
            headers,
            body,
        } = request;
        let mut builder = HttpRequest::builder().method(method).uri(url);
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
        let length = match &body {
            Body::Empty => None,
            Body::Bytes(bytes) => Some(bytes.len() as u64),
            Body::Stream { length, .. } => Some(*length),
        };
        if let Some(length) = length {
            builder = builder.header("Content-Length", length);
        }
        let (parts, ()) = builder
            .body(())
            .map_err(|_| CiError::new("Registry request has an incorrect format."))?
            .into_parts();
        match body {
            Body::Empty => self.run(parts, SendBody::none()),
            Body::Bytes(mut bytes) => self.run(parts, bytes.as_body()),
            Body::Stream { mut reader, .. } => self.run(parts, SendBody::from_reader(&mut reader)),
        }
    }
}
