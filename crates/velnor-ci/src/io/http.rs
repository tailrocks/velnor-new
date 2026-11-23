// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::time::Duration;

use ureq::RequestBuilder;
use ureq::http::{Response, StatusCode};

use crate::error::CiError;
use crate::github::{GitHub, GitHubWrite};
use crate::release_family::GitHubPost;

pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
const BODY_LIMIT: u64 = 512 * 1024 * 1024;
const ERROR_BODY_LIMIT: u64 = 64 * 1024;
const DETAIL_LIMIT: usize = 500;
const RESPONSE_HEADERS: &[&str] = &["retry-after", "x-ratelimit-remaining", "x-ratelimit-reset"];

#[derive(Debug)]
pub(super) struct HttpGitHub {
    agent: ureq::Agent,
    api: String,
    token: String,
}

impl HttpGitHub {
    pub(super) fn new(api: String, token: String, timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(crate::trusted_roots::configured_roots())
                    .build(),
            )
            .build()
            .into();
        Self { agent, api, token }
    }

    fn signed<B>(&self, request: RequestBuilder<B>) -> RequestBuilder<B> {
        request
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "velnor-ci")
    }
}

fn body(
    what: &str,
    response: Result<Response<ureq::Body>, ureq::Error>,
) -> Result<String, CiError> {
    let mut response = response.map_err(|e| transport_error(what, &e))?;
    if is_failure(response.status()) {
        return Err(status_error(what, &mut response));
    }
    let bytes = super::read_limited(response.body_mut().as_reader(), BODY_LIMIT)
        .map_err(|e| CiError::new(format!("{what}: {e}")))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn transport_error(what: &str, error: &ureq::Error) -> CiError {
    let message = format!("{what}: {error}");
    if matches!(
        error,
        ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
    ) {
        CiError::transient(message)
    } else {
        CiError::new(message)
    }
}

fn is_failure(status: StatusCode) -> bool {
    status.is_client_error() || status.is_server_error()
}

fn status_error(what: &str, response: &mut Response<ureq::Body>) -> CiError {
    let status = response.status().as_u16();
    let mut raw = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(ERROR_BODY_LIMIT)
        .read_to_end(&mut raw)
        .ok();
    let mut details = describe_body(&raw);
    for name in RESPONSE_HEADERS.iter().copied() {
        if let Some(value) = response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
        {
            details.push(format!("{name}: {}", clean(value)));
        }
    }
    let suffix = if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    };
    let message = format!("{what}: http status: {status}{suffix}");
    if matches!(status, 404 | 410) {
        CiError::missing(message)
    } else if status == 429 || status >= 500 {
        CiError::transient(message)
    } else {
        CiError::new(message)
    }
}

fn describe_body(raw: &[u8]) -> Vec<String> {
    if let Ok(serde_json::Value::Object(fields)) = serde_json::from_slice(raw) {
        let details: Vec<String> = ["message", "documentation_url"]
            .into_iter()
            .filter_map(|key| {
                fields
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .map(|text| format!("{key}: {}", clean(text)))
            })
            .collect();
        if !details.is_empty() {
            return details;
        }
    }
    let text = clean(&String::from_utf8_lossy(raw));
    if text.is_empty() {
        Vec::new()
    } else {
        vec![format!("body: {text}")]
    }
}

fn clean(text: &str) -> String {
    let printable: String = text.chars().filter(|c| !c.is_control()).collect();
    crate::escape::inert(clip(&printable))
}

fn clip(text: &str) -> &str {
    let mut end = text.len().min(DETAIL_LIMIT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl GitHub for HttpGitHub {
    fn get(&self, path: &str) -> Result<String, CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("GET {path}"),
            self.signed(self.agent.get(&url)).call(),
        )
    }
}

impl GitHubPost for HttpGitHub {
    fn post(&self, path: &str, json: &str) -> Result<String, CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("POST {path}"),
            self.signed(self.agent.post(&url))
                .header("Content-Type", "application/json")
                .send(json),
        )
    }
}

impl GitHubWrite for HttpGitHub {
    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn upload(&self, url: &str, bytes: &[u8]) -> Result<String, CiError> {
        body(
            &format!("POST {url}"),
            self.signed(self.agent.post(url))
                .header("Content-Type", "application/octet-stream")
                .send(bytes),
        )
    }

    fn patch(&self, path: &str, json: &str) -> Result<String, CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("PATCH {path}"),
            self.signed(self.agent.patch(&url))
                .header("Content-Type", "application/json")
                .send(json),
        )
    }

    fn delete(&self, path: &str) -> Result<(), CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("DELETE {path}"),
            self.signed(self.agent.delete(&url)).call(),
        )
        .map(drop)
    }
}

impl HttpGitHub {
    /// Open the content of a release asset. The API answers with a redirect to signed storage,
    /// and the agent drops the `Authorization` header when it follows it.
    pub(super) fn open_asset(&self, path: &str) -> Result<Box<dyn Read + Send>, CiError> {
        let url = format!("{}{path}", self.api);
        let what = format!("GET {path}");
        let mut response = self
            .agent
            .get(&url)
            .config()
            .redirect_auth_headers(ureq::config::RedirectAuthHeaders::Never)
            .build()
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/octet-stream")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "velnor-ci")
            .call()
            .map_err(|e| CiError::new(format!("{what}: {e}")))?;
        if is_failure(response.status()) {
            return Err(status_error(&what, &mut response));
        }
        Ok(Box::new(response.into_body().into_reader()))
    }

    /// The body of a GET of `path`, `None` on any failure or a body over `limit` bytes.
    pub(super) fn get_bytes(&self, path: &str, limit: u64) -> Option<Vec<u8>> {
        let url = format!("{}{path}", self.api);
        let mut response = self.signed(self.agent.get(&url)).call().ok()?;
        if is_failure(response.status()) {
            return None;
        }
        super::read_limited(response.body_mut().as_reader(), limit).ok()
    }
}

pub(super) fn trusted_api(api: &str) -> bool {
    let Ok(uri) = api.parse::<ureq::http::Uri>() else {
        return false;
    };
    match uri.scheme_str() {
        Some("https") => uri.host().is_some(),
        Some("http") => matches!(uri.host(), Some("127.0.0.1" | "localhost")),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    use super::*;

    fn api_answering(status: u16) -> HttpGitHub {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let api = format!("http://{}", listener.local_addr().expect("addr"));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line != "\r\n" {
                    line.clear();
                }
                let mut stream = &stream;
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .ok();
            }
        });
        HttpGitHub::new(api, "t".to_owned(), Duration::from_secs(5))
    }

    #[test]
    fn server_errors_and_rate_limits_are_transient_and_other_statuses_are_not() {
        for status in [500, 502, 503, 504, 429] {
            let api = api_answering(status);
            assert!(
                api.get("/x").expect_err("failure").is_transient(),
                "GET {status}"
            );
            assert!(
                api.post("/x", "{}").expect_err("failure").is_transient(),
                "POST {status}"
            );
        }
        for status in [400, 401, 403, 404, 410, 422] {
            let api = api_answering(status);
            assert!(
                !api.get("/x").expect_err("failure").is_transient(),
                "GET {status}"
            );
            assert!(
                !api.post("/x", "{}").expect_err("failure").is_transient(),
                "POST {status}"
            );
        }
    }

    #[test]
    fn a_refused_connection_is_transient() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let client = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let nobody_listens = client.local_addr().expect("addr");
        let api = HttpGitHub::new(
            format!("http://{nobody_listens}"),
            "t".to_owned(),
            Duration::from_secs(5),
        );
        let error = api.get("/x").expect_err("failure");
        assert!(error.to_string().contains("refused"), "{error}");
        assert!(error.is_transient(), "{error}");
    }
}
