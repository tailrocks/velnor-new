// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use ureq::RequestBuilder;
use ureq::http::Response;

use crate::error::CiError;
use crate::github::{GitHub, GitHubWrite};

pub(super) const PLAN_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
const BODY_LIMIT: u64 = 512 * 1024 * 1024;

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
            .build()
            .into();
        Self { agent, api, token }
    }

    fn signed<B>(&self, request: RequestBuilder<B>) -> RequestBuilder<B> {
        self.accepting(request, "application/vnd.github+json")
    }

    fn accepting<B>(&self, request: RequestBuilder<B>, accept: &str) -> RequestBuilder<B> {
        request
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", accept)
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "velnor-ci")
    }
}

fn body(
    what: &str,
    response: Result<Response<ureq::Body>, ureq::Error>,
) -> Result<String, CiError> {
    let mut response = response.map_err(|e| CiError::new(format!("{what}: {e}")))?;
    let bytes = super::read_limited(response.body_mut().as_reader(), BODY_LIMIT)
        .map_err(|e| CiError::new(format!("{what}: {e}")))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

impl GitHub for HttpGitHub {
    fn get(&self, path: &str) -> Result<String, CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("GET {path}"),
            self.signed(self.agent.get(&url)).call(),
        )
    }

    fn get_raw(&self, path: &str) -> Result<String, CiError> {
        let url = format!("{}{path}", self.api);
        body(
            &format!("GET {path}"),
            self.accepting(self.agent.get(&url), "application/vnd.github.raw+json")
                .call(),
        )
    }
}

impl GitHubWrite for HttpGitHub {
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
