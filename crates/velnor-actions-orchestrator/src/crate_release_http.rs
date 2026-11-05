// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;
use std::{cell::RefCell, collections::BTreeMap, io::Read};

use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

mod graphql;
mod journal;
mod qualification;
mod routes;
mod validation;
pub(crate) use routes::encode;
use routes::{optional_route, valid_headers, valid_post_route, valid_route, valid_sha};
use validation::{
    journal_tag_hash, no_store, parse_json, response_etag, validate_journal_name,
    validate_journal_tag_response, validate_repository, validate_write,
};

const JSON_LIMIT: u64 = 8 * 1024 * 1024;
const JOURNAL_BLOB_JSON_LIMIT: u64 = 48 * 1024 * 1024;
const MAX_VALIDATED_RESPONSES: usize = 64;
const MAX_VALIDATED_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Clone)]
struct ValidatedResponse {
    etag: String,
    body: Value,
}

pub struct GitHubAdmissionHttp {
    token: String,
    timeout: std::time::Duration,
    origin: String,
    responses: RefCell<BTreeMap<(String, u64), ValidatedResponse>>,
}

impl std::fmt::Debug for GitHubAdmissionHttp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitHubAdmissionHttp")
            .finish_non_exhaustive()
    }
}

fn has_unverified_pagination(route: &str, headers: &ureq::http::HeaderMap) -> bool {
    let unpaginated_pulls = route.split_once('?').is_some_and(|(path, query)| {
        path.ends_with("/pulls")
            && path.bytes().filter(|byte| *byte == b'/').count() == 3
            && !query.contains("&page=")
    });
    (route.contains("/git/matching-refs/")
        || unpaginated_pulls
        || (route.contains("/jobs?") && !route.contains("&page=")))
        && headers.contains_key("link")
}

fn rehearsal_transport_error(error: &ureq::Error) -> OrchestratorError {
    #[cfg(test)]
    match error {
        ureq::Error::Io(io) => {
            eprintln!("fixture HTTP I/O: {:?} {:?}", io.kind(), io.raw_os_error());
        }
        ureq::Error::Timeout(timeout) => {
            eprintln!("fixture HTTP timeout: {timeout:?}");
        }
        _ => {
            eprintln!("fixture HTTP class: {:?}", std::mem::discriminant(error));
        }
    }
    let _ = error;
    internal("crate_release_http_transport")
}

impl GitHubAdmissionHttp {
    pub fn new(token: String, timeout: std::time::Duration) -> Result<Self, OrchestratorError> {
        if token.is_empty()
            || timeout.is_zero()
            || timeout > std::time::Duration::from_secs(180)
            || token.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(internal("crate_release_http_token"));
        }
        #[cfg(test)]
        let origin = test_loopback_origin("VELNOR_TEST_GITHUB_ORIGIN")?
            .unwrap_or_else(|| "https://api.github.com".to_owned());
        #[cfg(not(test))]
        let origin = "https://api.github.com".to_owned();
        Ok(Self {
            token,
            timeout,
            origin,
            responses: RefCell::new(BTreeMap::new()),
        })
    }

    #[cfg(test)]
    pub(crate) fn test_client(origin: &str) -> Result<Self, OrchestratorError> {
        Ok(Self {
            token: "test-credential".to_owned(),
            timeout: std::time::Duration::from_secs(15),
            origin: parse_test_loopback_origin(origin)?,
            responses: RefCell::new(BTreeMap::new()),
        })
    }

    pub fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.request(route, false)?
            .ok_or_else(|| internal("crate_release_http_status"))
    }

    pub fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        if !optional_route(route) {
            return Err(internal("crate_release_http_route"));
        }
        let result = self.request(route, true)?;
        if result.is_none() {
            self.verify_repository_access(route)?;
        }
        Ok(result)
    }

    pub fn create_tag(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        validate_write(repository, tag, source_sha)?;
        let expected_ref = format!("refs/tags/{tag}");
        let body = serde_json::json!({"ref": expected_ref, "sha": source_sha});
        let result = self.post(&format!("repos/{repository}/git/refs"), &body)?;
        if result["ref"].as_str() != Some(expected_ref.as_str())
            || result["object"]["type"].as_str() != Some("commit")
            || result["object"]["sha"].as_str() != Some(source_sha)
        {
            return Err(internal("crate_release_http_write_identity"));
        }
        Ok(())
    }

    pub fn create_release(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        validate_write(repository, tag, source_sha)?;
        let body = serde_json::json!({"tag_name": tag, "name": tag, "target_commitish": source_sha, "make_latest": "false"});
        let result = self.post(&format!("repos/{repository}/releases"), &body)?;
        let expected = format!("https://github.com/{repository}/releases/tag/{tag}");
        let expected_encoded = format!(
            "https://github.com/{repository}/releases/tag/{}",
            encode(tag)
        );
        if result["tag_name"].as_str() != Some(tag)
            || result["draft"].as_bool() != Some(false)
            || !matches!(result["html_url"].as_str(), Some(url) if url == expected || url == expected_encoded)
        {
            return Err(internal("crate_release_http_write_identity"));
        }
        Ok(())
    }

    pub(crate) fn post(&self, route: &str, body: &Value) -> Result<Value, OrchestratorError> {
        if !valid_post_route(route) {
            return Err(internal("crate_release_http_route"));
        }
        let deadline = Instant::now() + self.timeout;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(internal("crate_release_http_deadline"));
        }
        let body =
            serde_json::to_vec(body).map_err(|_| internal("crate_release_http_write_body"))?;
        let response = crate::api::github_evidence_http::agent()?
            .post(format!("{}/{route}", self.origin))
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Accept-Encoding", "identity")
            .header("Content-Type", "application/json")
            .header("User-Agent", "velnor-actions-crate-release")
            .config()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .send(body.as_slice())
            .map_err(|_| internal("crate_release_http_transport"))?;
        if response.status().as_u16() != 201 {
            return Err(internal("crate_release_http_write_status"));
        }
        parse_json(response, deadline, JSON_LIMIT)
    }

    fn request(&self, route: &str, optional: bool) -> Result<Option<Value>, OrchestratorError> {
        self.request_with_limit(route, optional, JSON_LIMIT)
    }

    fn request_with_limit(
        &self,
        route: &str,
        optional: bool,
        limit: u64,
    ) -> Result<Option<Value>, OrchestratorError> {
        if !valid_route(route) {
            return Err(internal("crate_release_http_route"));
        }
        let deadline = Instant::now() + self.timeout;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(internal("crate_release_http_deadline"));
        }
        let url = format!("{}/{route}", self.origin);
        let key = (route.to_owned(), limit);
        let previous = self.responses.borrow().get(&key).cloned();
        let mut request = crate::api::github_evidence_http::agent()?
            .get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Accept-Encoding", "identity")
            .header("User-Agent", "velnor-actions-crate-release");
        if let Some(entry) = &previous {
            request = request.header("If-None-Match", &entry.etag);
        }
        let response = request
            .config()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|error| rehearsal_transport_error(&error))?;
        if has_unverified_pagination(route, response.headers()) {
            self.responses.borrow_mut().remove(&key);
            return Err(internal("crate_release_http_pagination_unverified"));
        }
        if response.status().as_u16() == 304 {
            let entry = previous.ok_or_else(|| internal("crate_release_http_response"))?;
            if no_store(response.headers())
                || response.headers().get_all("etag").iter().count() > 1
                || response
                    .headers()
                    .get("etag")
                    .is_some_and(|etag| etag.to_str().ok() != Some(entry.etag.as_str()))
                || Instant::now() >= deadline
            {
                self.responses.borrow_mut().remove(&key);
                return Err(internal("crate_release_http_response"));
            }
            return Ok(Some(entry.body));
        }
        self.responses.borrow_mut().remove(&key);
        if optional && response.status().as_u16() == 404 {
            return Ok(None);
        }
        if response.status().as_u16() != 200 {
            return Err(internal("crate_release_http_response"));
        }
        let etag = response_etag(response.headers());
        let body = parse_json(response, deadline, limit)?;
        if let Some(etag) = etag {
            let small = serde_json::to_vec(&body)
                .is_ok_and(|bytes| bytes.len() <= MAX_VALIDATED_RESPONSE_BYTES);
            let mut cache = self.responses.borrow_mut();
            if small && cache.len() < MAX_VALIDATED_RESPONSES {
                cache.insert(
                    key,
                    ValidatedResponse {
                        etag,
                        body: body.clone(),
                    },
                );
            }
        }
        Ok(Some(body))
    }

    fn verify_repository_access(&self, route: &str) -> Result<(), OrchestratorError> {
        let mut parts = route.split('/');
        if parts.next() != Some("repos") {
            return Err(internal("crate_release_http_route"));
        }
        let owner = parts
            .next()
            .ok_or_else(|| internal("crate_release_http_route"))?;
        let repo = parts
            .next()
            .ok_or_else(|| internal("crate_release_http_route"))?;
        let prefix = format!("repos/{owner}/{repo}");
        let info = self.get(&prefix)?;
        if info["full_name"].as_str() != Some(format!("{owner}/{repo}").as_str()) {
            return Err(internal("crate_release_http_access"));
        }
        let branch = info["default_branch"]
            .as_str()
            .ok_or_else(|| internal("crate_release_http_access"))?;
        let encoded = encode(branch);
        let reference = self.get(&format!("{prefix}/git/ref/heads/{encoded}"))?;
        let sha = reference["object"]["sha"].as_str().unwrap_or_default();
        if reference["ref"].as_str() != Some(format!("refs/heads/{branch}").as_str())
            || reference["object"]["type"].as_str() != Some("commit")
            || !valid_sha(sha)
        {
            return Err(internal("crate_release_http_access"));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn test_loopback_origin(name: &str) -> Result<Option<String>, OrchestratorError> {
    let Some(origin) = std::env::var_os(name) else {
        return Ok(None);
    };
    let origin = origin
        .into_string()
        .map_err(|_| internal("crate_release_test_origin"))?;
    parse_test_loopback_origin(&origin).map(Some)
}

#[cfg(test)]
fn parse_test_loopback_origin(origin: &str) -> Result<String, OrchestratorError> {
    let port = origin
        .strip_prefix("http://127.0.0.1:")
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| internal("crate_release_test_origin"))?;
    Ok(format!("http://127.0.0.1:{port}"))
}

#[cfg(test)]
mod tests;
