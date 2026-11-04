// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::time::Instant;

use serde_json::Value;

use crate::{OrchestratorError, internal::internal};

const JSON_LIMIT: u64 = 8 * 1024 * 1024;
const JOURNAL_BLOB_JSON_LIMIT: u64 = 48 * 1024 * 1024;

pub struct GitHubAdmissionHttp {
    token: String,
    deadline: Instant,
    origin: String,
}

impl std::fmt::Debug for GitHubAdmissionHttp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitHubAdmissionHttp")
            .finish_non_exhaustive()
    }
}

impl GitHubAdmissionHttp {
    pub(crate) fn check_journal_repository_access(
        &self,
        repository: &str,
    ) -> Result<(), OrchestratorError> {
        validate_repository(repository)?;
        self.verify_repository_access(&format!(
            "repos/{repository}/git/ref/tags/velnor-receipt%2F1%2F1%2Fpublication-started"
        ))
    }
    pub fn list_journal_operations(
        &self,
        repository: &str,
        run_id: u64,
        attempt: u32,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        validate_repository(repository)?;
        if run_id == 0 || attempt == 0 {
            return Err(internal("crate_release_http_write_input"));
        }
        let prefix = format!("velnor-receipt/{run_id}/{attempt}/operation-");
        let route = format!(
            "repos/{repository}/git/matching-refs/tags/{}",
            encode(&prefix)
        );
        let body = self.get(&route)?;
        let entries = body
            .as_array()
            .filter(|items| items.len() <= 1000)
            .ok_or_else(|| internal("crate_release_http_response"))?;
        let mut unique = std::collections::BTreeMap::new();
        for entry in entries {
            let reference = entry["ref"]
                .as_str()
                .ok_or_else(|| internal("crate_release_http_response"))?;
            let name = reference
                .strip_prefix("refs/tags/")
                .ok_or_else(|| internal("crate_release_http_response"))?;
            if !name.starts_with(&prefix)
                || validate_journal_name(name).is_err()
                || entry["object"]["type"].as_str() != Some("tag")
                || !entry["object"]["sha"].as_str().is_some_and(valid_sha)
                || unique
                    .insert(
                        reference.to_owned(),
                        entry["object"]["sha"]
                            .as_str()
                            .ok_or_else(|| internal("crate_release_http_response"))?
                            .to_owned(),
                    )
                    .is_some()
            {
                return Err(internal("crate_release_http_response"));
            }
        }
        Ok(unique.into_iter().collect())
    }

    pub fn create_journal_tag(
        &self,
        repository: &str,
        tag: &str,
        blob_sha: &str,
    ) -> Result<String, OrchestratorError> {
        validate_repository(repository)?;
        let expected = journal_tag_hash(tag, blob_sha)?;
        let body = serde_json::json!({"tag":tag,"message":"velnor-release-receipt-v1","object":blob_sha,"type":"blob","tagger":{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"}});
        let response = self.post(&format!("repos/{repository}/git/tags"), &body)?;
        validate_journal_tag_response(&response, &expected)?;
        if response["tag"].as_str() != Some(tag)
            || response["object"]["sha"].as_str() != Some(blob_sha)
        {
            return Err(internal("crate_release_http_write_identity"));
        }
        Ok(expected)
    }
    pub fn create_journal_ref(
        &self,
        repository: &str,
        reference: &str,
        tag_sha: &str,
    ) -> Result<(), OrchestratorError> {
        validate_repository(repository)?;
        let name = reference
            .strip_prefix("refs/tags/")
            .ok_or_else(|| internal("crate_release_http_write_input"))?;
        validate_journal_name(name)?;
        if !valid_sha(tag_sha) {
            return Err(internal("crate_release_http_write_input"));
        }
        let response = self.post(
            &format!("repos/{repository}/git/refs"),
            &serde_json::json!({"ref":reference,"sha":tag_sha}),
        )?;
        if response["ref"].as_str() != Some(reference)
            || response["object"]["type"].as_str() != Some("tag")
            || response["object"]["sha"].as_str() != Some(tag_sha)
        {
            return Err(internal("crate_release_http_write_identity"));
        }
        Ok(())
    }
    pub fn get_journal_ref(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<Option<String>, OrchestratorError> {
        validate_repository(repository)?;
        let name = reference
            .strip_prefix("refs/tags/")
            .ok_or_else(|| internal("crate_release_http_write_input"))?;
        validate_journal_name(name)?;
        let response =
            self.get_optional(&format!("repos/{repository}/git/ref/tags/{}", encode(name)))?;
        response
            .map(|item| {
                let sha = item["object"]["sha"]
                    .as_str()
                    .filter(|sha| valid_sha(sha))
                    .ok_or_else(|| internal("crate_release_http_response"))?;
                if item["ref"].as_str() != Some(reference)
                    || item["object"]["type"].as_str() != Some("tag")
                {
                    return Err(internal("crate_release_http_response"));
                }
                Ok(sha.to_owned())
            })
            .transpose()
    }
    pub fn get_journal_tag(&self, repository: &str, sha: &str) -> Result<Value, OrchestratorError> {
        validate_repository(repository)?;
        if !valid_sha(sha) {
            return Err(internal("crate_release_http_write_input"));
        }
        let response = self.get(&format!("repos/{repository}/git/tags/{sha}"))?;
        validate_journal_tag_response(&response, sha)?;
        Ok(response)
    }
    pub fn get_journal_blob(
        &self,
        repository: &str,
        sha: &str,
    ) -> Result<Vec<u8>, OrchestratorError> {
        use base64::Engine as _;
        validate_repository(repository)?;
        if !valid_sha(sha) {
            return Err(internal("crate_release_http_write_input"));
        }
        let response = self
            .request_with_limit(
                &format!("repos/{repository}/git/blobs/{sha}"),
                false,
                JOURNAL_BLOB_JSON_LIMIT,
            )?
            .ok_or_else(|| internal("crate_release_http_status"))?;
        let invalid = || internal("crate_release_http_response");
        if response["sha"].as_str() != Some(sha) || response["encoding"].as_str() != Some("base64")
        {
            return Err(invalid());
        }
        let encoded = response["content"]
            .as_str()
            .ok_or_else(invalid)?
            .replace('\n', "");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid())?;
        if bytes.len() > 32 * 1024 * 1024
            || response["size"].as_u64() != Some(bytes.len() as u64)
            || crate::release_policy_admission::git_blob_sha1(&bytes)? != sha
        {
            return Err(invalid());
        }
        Ok(bytes)
    }
    pub fn create_journal_blob(
        &self,
        repository: &str,
        bytes: &[u8],
    ) -> Result<String, OrchestratorError> {
        use base64::Engine as _;
        validate_repository(repository)?;
        if bytes.is_empty() || bytes.len() > 32 * 1024 * 1024 {
            return Err(internal("crate_release_journal_blob_input"));
        }
        let sha = crate::release_policy_admission::git_blob_sha1(bytes)?;
        let body = serde_json::json!({"content":base64::engine::general_purpose::STANDARD.encode(bytes),"encoding":"base64"});
        let response = self.post(&format!("repos/{repository}/git/blobs"), &body)?;
        if response["sha"].as_str() != Some(sha.as_str()) {
            return Err(internal("crate_release_http_write_identity"));
        }
        Ok(sha)
    }
    pub fn new(token: String, deadline: Instant) -> Result<Self, OrchestratorError> {
        if token.is_empty() || token.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(internal("crate_release_http_token"));
        }
        #[cfg(test)]
        let origin = test_loopback_origin("VELNOR_TEST_GITHUB_ORIGIN")?
            .unwrap_or_else(|| "https://api.github.com".to_owned());
        #[cfg(not(test))]
        let origin = "https://api.github.com".to_owned();
        Ok(Self {
            token,
            deadline,
            origin,
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
        let body =
            serde_json::json!({"tag_name": tag, "name": tag, "target_commitish": source_sha});
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

    fn post(&self, route: &str, body: &Value) -> Result<Value, OrchestratorError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
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
        parse_json(response, self.deadline, JSON_LIMIT)
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
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(internal("crate_release_http_deadline"));
        }
        let url = format!("{}/{route}", self.origin);
        let response = crate::api::github_evidence_http::agent()?
            .get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Accept-Encoding", "identity")
            .header("User-Agent", "velnor-actions-crate-release")
            .config()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|_| internal("crate_release_http_transport"))?;
        if optional && response.status().as_u16() == 404 {
            return Ok(None);
        }
        if response.status().as_u16() != 200 {
            return Err(internal("crate_release_http_response"));
        }
        if route.contains("/git/matching-refs/") && response.headers().contains_key("link") {
            return Err(internal("crate_release_http_pagination_unverified"));
        }
        parse_json(response, self.deadline, limit).map(Some)
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

fn parse_json(
    response: ureq::http::Response<ureq::Body>,
    deadline: Instant,
    limit: u64,
) -> Result<Value, OrchestratorError> {
    if !valid_headers(response.headers(), limit) {
        return Err(internal("crate_release_http_response"));
    }
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| internal("crate_release_http_body"))?;
    if bytes.is_empty() || bytes.len() as u64 > limit || Instant::now() >= deadline {
        return Err(internal("crate_release_http_body"));
    }
    serde_json::from_slice(&bytes).map_err(|_| internal("crate_release_http_json"))
}

fn validate_journal_name(name: &str) -> Result<(), OrchestratorError> {
    let parts: Vec<_> = name.split('/').collect();
    if !matches!(parts.as_slice(), ["velnor-receipt", run, attempt, phase] if canonical_positive(run) && canonical_positive(attempt) && crate::crate_release_journal::valid_journal_phase(phase))
    {
        return Err(internal("crate_release_http_write_input"));
    }
    Ok(())
}

fn canonical_positive(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|number| number > 0 && number.to_string() == value)
}

fn journal_tag_hash(tag: &str, blob_sha: &str) -> Result<String, OrchestratorError> {
    use sha1_checked::{Digest as _, Sha1};
    validate_journal_name(tag)?;
    if !valid_sha(blob_sha) {
        return Err(internal("crate_release_http_write_input"));
    }
    let raw = format!(
        "object {blob_sha}\ntype blob\ntag {tag}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1"
    );
    let mut hasher = Sha1::new();
    hasher.update(format!("tag {}\0", raw.len()).as_bytes());
    hasher.update(raw.as_bytes());
    let digest = hasher.try_finalize();
    if digest.has_collision() {
        return Err(internal("crate_release_http_write_identity"));
    }
    Ok(digest
        .hash()
        .iter()
        .fold(String::new(), |mut output, byte| {
            output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
            output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
            output
        }))
}

fn validate_journal_tag_response(
    value: &Value,
    expected_sha: &str,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_http_write_identity");
    let name = value["tag"].as_str().ok_or_else(invalid)?;
    let blob = value["object"]["sha"].as_str().ok_or_else(invalid)?;
    if value["sha"].as_str() != Some(expected_sha)
        || journal_tag_hash(name, blob)? != expected_sha
        || value["object"]["type"].as_str() != Some("blob")
        || value["message"].as_str() != Some("velnor-release-receipt-v1")
        || value["tagger"]["name"].as_str() != Some("Velnor Release")
        || value["tagger"]["email"].as_str() != Some("release@velnor.invalid")
        || value["tagger"]["date"].as_str() != Some("1970-01-01T00:00:00Z")
        || value["verification"]["verified"].as_bool() != Some(false)
        || value["verification"]["reason"].as_str() != Some("unsigned")
        || !value["verification"]["signature"].is_null()
        || !value["verification"]["payload"].is_null()
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_repository(repository: &str) -> Result<(), OrchestratorError> {
    let Some((owner, repo)) = repository.split_once('/') else {
        return Err(internal("crate_release_http_write_input"));
    };
    if !velnor_actions_contract::config::is_valid_github_owner(owner)
        || !velnor_actions_contract::config::is_valid_github_repo_name(repo)
    {
        return Err(internal("crate_release_http_write_input"));
    }
    Ok(())
}

fn validate_write(repository: &str, tag: &str, sha: &str) -> Result<(), OrchestratorError> {
    let Some((owner, repo)) = repository.split_once('/') else {
        return Err(internal("crate_release_http_write_input"));
    };
    if !velnor_actions_contract::config::is_valid_github_owner(owner)
        || !velnor_actions_contract::config::is_valid_github_repo_name(repo)
        || !valid_sha(sha)
        || tag.is_empty()
        || tag.starts_with('.')
        || tag.ends_with('.')
        || tag
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension == "lock")
        || tag.contains("..")
        || !tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
    {
        return Err(internal("crate_release_http_write_input"));
    }
    Ok(())
}

fn valid_headers(headers: &ureq::http::HeaderMap, limit: u64) -> bool {
    let mut encodings = headers.get_all("content-encoding").iter();
    match (encodings.next(), encodings.next()) {
        (None, None) => {}
        (Some(value), None)
            if value
                .to_str()
                .is_ok_and(|value| value.eq_ignore_ascii_case("identity")) => {}
        _ => return false,
    }
    let mut lengths = headers.get_all("content-length").iter();
    match (lengths.next(), lengths.next()) {
        (None, None) => true,
        (Some(value), None) => value
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n <= limit),
        _ => false,
    }
}

fn encode(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            result.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            result.push('%');
            result.push(char::from(HEX[usize::from(byte >> 4)]));
            result.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    result
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_segment(value: &str) -> bool {
    if value.is_empty() || value.len() > 768 {
        return false;
    }
    let mut decoded = Vec::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let (Some(a), Some(b)) = (bytes.next(), bytes.next()) else {
                return false;
            };
            let Some(a) = char::from(a).to_digit(16) else {
                return false;
            };
            let Some(b) = char::from(b).to_digit(16) else {
                return false;
            };
            let value = u8::try_from(a * 16 + b).unwrap_or(0);
            if value == b'%' || value.is_ascii_control() || matches!(value, b'?' | b'#' | b'\\') {
                return false;
            }
            decoded.push(value);
        } else if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'+') {
            decoded.push(byte);
        } else {
            return false;
        }
    }
    let Ok(text) = std::str::from_utf8(&decoded) else {
        return false;
    };
    !text
        .split('/')
        .any(|part| part == "." || part == ".." || part.is_empty())
}

fn numeric(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|n| n > 0)
}

fn page_query(query: &str) -> bool {
    query
        .strip_prefix("per_page=100&page=")
        .is_some_and(|v| v.parse::<u32>().is_ok_and(|n| (1..=100).contains(&n)))
}

fn valid_route(route: &str) -> bool {
    if route.len() > 4096 || !route.is_ascii() || route.contains('#') {
        return false;
    }
    let (path, query) = route.split_once('?').map_or((route, ""), |(p, q)| (p, q));
    if query.contains('?') {
        return false;
    }
    let segments: Vec<&str> = path.split('/').collect();
    if let ["users", user] = segments.as_slice() {
        return query.is_empty() && valid_segment(user);
    }
    let ["repos", owner, repo, rest @ ..] = segments.as_slice() else {
        return false;
    };
    if !velnor_actions_contract::config::is_valid_github_owner(owner)
        || !velnor_actions_contract::config::is_valid_github_repo_name(repo)
    {
        return false;
    }
    match rest {
        [] => query.is_empty(),
        ["actions", "runs", id] | ["actions", "runs", id, "approvals"] => {
            numeric(id) && query.is_empty()
        }
        ["actions", "runs", id, "attempts", attempt] => {
            numeric(id) && numeric(attempt) && query.is_empty()
        }
        ["actions", "runs", id, "artifacts"] => {
            numeric(id) && (query == "per_page=100" || page_query(query))
        }
        ["actions", "artifacts", id] | ["rulesets", id] => numeric(id) && query.is_empty(),
        ["actions", "runs"] => query
            .strip_prefix("head_sha=")
            .and_then(|q| q.split_once('&'))
            .is_some_and(|(sha, q)| valid_sha(sha) && page_query(q)),
        ["compare", range] => {
            range
                .split_once("...")
                .is_some_and(|(left, right)| valid_sha(left) && valid_segment(right))
                && query.is_empty()
        }
        ["environments", name]
        | ["git", "ref", "heads" | "tags", name]
        | ["collaborators", name, "permission"]
        | ["releases", "tags", name] => valid_segment(name) && query.is_empty(),
        ["environments", name, "deployment-branch-policies"] => {
            valid_segment(name) && query == "per_page=100"
        }
        ["git", "matching-refs", "tags", prefix] => valid_segment(prefix) && query.is_empty(),
        ["git", "tags" | "commits" | "blobs", sha] => valid_sha(sha) && query.is_empty(),
        ["commits", sha, "pulls"] => valid_sha(sha) && page_query(query),
        ["commits", sha, "check-runs"] => {
            valid_sha(sha) && query.strip_prefix("filter=all&").is_some_and(page_query)
        }
        ["pulls", id, "reviews"] => numeric(id) && page_query(query),
        ["rulesets"] => query == "targets=tag&includes_parents=true&per_page=100",
        _ => false,
    }
}

fn optional_route(route: &str) -> bool {
    if !valid_route(route) {
        return false;
    }
    let parts: Vec<&str> = route.split('/').collect();
    matches!(
        parts.as_slice(),
        ["repos", _, _, "git", "ref", "tags", _] | ["repos", _, _, "releases", "tags", _]
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_transport_origins_are_loopback_only() {
        assert_eq!(
            super::parse_test_loopback_origin("http://127.0.0.1:8080")
                .ok()
                .as_deref(),
            Some("http://127.0.0.1:8080")
        );
        for invalid in [
            "https://api.github.com",
            "http://localhost:80",
            "http://127.0.0.1:0",
            "http://127.0.0.1:80/path",
            "http://127.0.0.1:80@evil.example",
            "http://127.0.0.2:80",
        ] {
            assert!(super::parse_test_loopback_origin(invalid).is_err());
        }
    }
    #[test]
    fn reads_exact_workflow_attempt_and_rejects_unsafe_attempt_routes() {
        let (origin, worker) = server(vec![reply("200 OK", r#"{"id":42}"#)]);
        let response = client(origin)
            .get("repos/example/demo-crate/actions/runs/42/attempts/1")
            .expect("attempt response");
        assert_eq!(response["id"], 42);
        assert!(
            worker.join().expect("server")[0]
                .contains("/repos/example/demo-crate/actions/runs/42/attempts/1")
        );
        for route in [
            "repos/example/demo-crate/actions/runs/42/attempts/1/extra",
            "repos/example/demo-crate/actions/runs/42/attempts/1?extra=1",
            "repos/example/demo-crate/actions/runs/42/attempts/nope",
        ] {
            assert!(!super::valid_route(route), "accepted {route}");
        }
    }

    #[test]
    fn operation_listing_uses_exact_prefix_and_rejects_malformed_refs() {
        let good = format!(
            r#"[{{"ref":"refs/tags/velnor-receipt/42/1/operation-1","object":{{"type":"tag","sha":"{}"}}}}]"#,
            "a".repeat(40)
        );
        let (origin, worker) = server(vec![reply("200 OK", &good)]);
        let result = client(origin)
            .list_journal_operations("example/demo-crate", 42, 1)
            .expect("listing");
        assert_eq!(
            result,
            [(
                "refs/tags/velnor-receipt/42/1/operation-1".to_owned(),
                "a".repeat(40)
            )]
        );
        let requests = worker.join().expect("server");
        assert!(requests[0].contains(
            "/repos/example/demo-crate/git/matching-refs/tags/velnor-receipt%2F42%2F1%2Foperation-"
        ));
        let linked = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nLink: <https://api.github.com/next>; rel=\"next\"\r\nConnection: close\r\n\r\n{good}",
            good.len()
        );
        for response in [
            linked,
            reply("403 Forbidden", "{}"),
            reply("429 Too Many Requests", "{}"),
            reply("500 Internal Server Error", "{}"),
            reply("302 Found", "{}"),
        ] {
            let (origin, worker) = server(vec![response]);
            assert!(
                client(origin)
                    .list_journal_operations("example/demo-crate", 42, 1)
                    .is_err()
            );
            worker.join().expect("server");
        }
        for bad in [
            good.replace("operation-1", "operation-01"),
            good.replace("operation-1", "operation-1001"),
            good.replace(&"a".repeat(40), "not-a-sha"),
            good.replace("\"tag\"", "\"commit\""),
            "{}".to_owned(),
            format!(
                "[{},{}]",
                &good[1..good.len() - 1],
                &good[1..good.len() - 1]
            ),
        ] {
            let (origin, worker) = server(vec![reply("200 OK", &bad)]);
            assert!(
                client(origin)
                    .list_journal_operations("example/demo-crate", 42, 1)
                    .is_err()
            );
            worker.join().expect("server");
        }
    }

    #[test]
    fn operation_names_are_bounded_and_canonical() {
        for value in [
            "velnor-receipt/42/1/operation-1",
            "velnor-receipt/42/1/operation-1000",
        ] {
            assert!(super::validate_journal_name(value).is_ok());
        }
        for value in [
            "velnor-receipt/42/1/operation-0",
            "velnor-receipt/42/1/operation-01",
            "velnor-receipt/42/1/operation-1001",
            "velnor-receipt/42/1/operation--1",
            "velnor-receipt/42/1/operation-1/extra",
        ] {
            assert!(super::validate_journal_name(value).is_err());
        }
    }

    use super::GitHubAdmissionHttp;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    const TOKEN: &str = "synthetic-test-credential";

    #[test]
    fn journal_adapter_reads_only_the_requested_canonical_tag() {
        use crate::{
            crate_release_github::NativeGitHubAdmissionRead, crate_release_journal::JournalIo as _,
            crate_release_journal_adapter::JournalAdapter,
        };
        use sha1_checked::{Digest as _, Sha1};
        let blob = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
        let name = "velnor-receipt/12/1/publication-started";
        let raw = format!(
            "object {blob}\ntype blob\ntag {name}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1"
        );
        let mut digest = Sha1::new();
        digest.update(format!("tag {}\0", raw.len()).as_bytes());
        digest.update(raw.as_bytes());
        let sha = digest
            .try_finalize()
            .hash()
            .iter()
            .fold(String::new(), |mut out, byte| {
                use std::fmt::Write as _;
                write!(&mut out, "{byte:02x}").expect("format");
                out
            });
        let tag = format!(
            r#"{{"sha":"{sha}","tag":"{name}","message":"velnor-release-receipt-v1","object":{{"sha":"{blob}","type":"blob"}},"tagger":{{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"}},"verification":{{"verified":false,"reason":"unsigned","signature":null,"payload":null}}}}"#
        );
        let (origin, worker) = server(vec![reply("200 OK", &tag)]);
        let adapter = JournalAdapter::new(NativeGitHubAdmissionRead {
            reader: client(origin),
        });
        assert_eq!(
            adapter
                .get_verified_tag_bytes("example/demo-crate", &sha)
                .expect("canonical tag"),
            raw.as_bytes()
        );
        let requests = worker.join().expect("server");
        assert!(requests[0].contains(&format!("/repos/example/demo-crate/git/tags/{sha}")));
    }

    #[test]
    fn journal_adapter_rejects_inconsistent_or_signed_tag_responses() {
        use crate::{
            crate_release_github::NativeGitHubAdmissionRead, crate_release_journal::JournalIo as _,
            crate_release_journal_adapter::JournalAdapter,
        };
        let sha = "a1dbc04588c3239fd6552dda558c5c5431a891c5";
        let valid = format!(
            r#"{{"sha":"{sha}","tag":"velnor-receipt/12/1/publication-started","message":"velnor-release-receipt-v1","object":{{"sha":"b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0","type":"blob"}},"tagger":{{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"}},"verification":{{"verified":false,"reason":"unsigned","signature":null,"payload":null}}}}"#
        );
        for body in [
            valid.replace("\"type\":\"blob\"", "\"type\":\"commit\""),
            valid.replace("\"verified\":false", "\"verified\":true"),
            valid.replace("1970-01-01T00:00:00Z", "1970-01-01T00:00:01Z"),
            "{}".to_owned(),
        ] {
            let (origin, worker) = server(vec![reply("200 OK", &body)]);
            let adapter = JournalAdapter::new(NativeGitHubAdmissionRead {
                reader: client(origin),
            });
            assert!(
                adapter
                    .get_verified_tag_bytes("example/demo-crate", sha)
                    .is_err()
            );
            assert_eq!(worker.join().expect("server").len(), 1);
        }
    }

    #[test]
    fn journal_transport_binds_tag_ref_and_blob() {
        exercise_journal_transport("velnor-receipt/12/1/publication-started");
        exercise_journal_transport("velnor-receipt/12/1/operation-1");
    }

    fn exercise_journal_transport(name: &str) {
        use sha1_checked::{Digest as _, Sha1};
        let blob_sha = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
        let raw = format!(
            "object {blob_sha}\ntype blob\ntag {name}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1"
        );
        let mut hasher = Sha1::new();
        hasher.update(format!("tag {}\0", raw.len()).as_bytes());
        hasher.update(raw.as_bytes());
        let tag_sha = hasher
            .try_finalize()
            .hash()
            .iter()
            .fold(String::new(), |mut out, byte| {
                use std::fmt::Write as _;
                write!(&mut out, "{byte:02x}").expect("format");
                out
            });
        let reference = format!("refs/tags/{name}");
        let tag = format!(
            r#"{{"sha":"{tag_sha}","tag":"{name}","message":"velnor-release-receipt-v1","object":{{"sha":"{blob_sha}","type":"blob"}},"tagger":{{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"}},"verification":{{"verified":false,"reason":"unsigned","signature":null,"payload":null}}}}"#
        );
        let reference_body =
            format!(r#"{{"ref":"{reference}","object":{{"sha":"{tag_sha}","type":"tag"}}}}"#);
        let blob =
            format!(r#"{{"sha":"{blob_sha}","encoding":"base64","size":5,"content":"aGVsbG8="}}"#);
        let (origin, worker) = server(vec![
            reply("201 Created", &tag),
            reply("201 Created", &reference_body),
            reply("200 OK", &reference_body),
            reply("200 OK", &tag),
            reply("200 OK", &blob),
        ]);
        let client = client(origin);
        assert_eq!(
            client
                .create_journal_tag("example/demo-crate", name, blob_sha)
                .expect("tag"),
            tag_sha
        );
        client
            .create_journal_ref("example/demo-crate", &reference, &tag_sha)
            .expect("ref");
        assert_eq!(
            client
                .get_journal_ref("example/demo-crate", &reference)
                .expect("read ref"),
            Some(tag_sha.clone())
        );
        assert_eq!(
            client
                .get_journal_tag("example/demo-crate", &tag_sha)
                .expect("read tag")["tag"],
            name
        );
        assert_eq!(
            client
                .get_journal_blob("example/demo-crate", blob_sha)
                .expect("read blob"),
            b"hello"
        );
        assert_eq!(worker.join().expect("server").len(), 5);
    }

    #[test]
    fn journal_blob_read_and_write_reject_wrong_content_and_errors() {
        let sha = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
        for body in [
            format!(r#"{{"sha":"{sha}","encoding":"base64","size":5,"content":"d29ybGQ="}}"#),
            format!(r#"{{"sha":"{sha}","encoding":"base64","size":6,"content":"aGVsbG8="}}"#),
        ] {
            let (origin, worker) = server(vec![reply("200 OK", &body)]);
            assert!(
                client(origin)
                    .get_journal_blob("example/demo-crate", sha)
                    .is_err()
            );
            worker.join().expect("server");
        }
        for status in [
            "409 Conflict",
            "422 Unprocessable Entity",
            "500 Internal Server Error",
        ] {
            let (origin, worker) = server(vec![reply(status, "{}")]);
            assert!(
                client(origin)
                    .create_journal_blob("example/demo-crate", b"hello")
                    .is_err()
            );
            assert_eq!(worker.join().expect("server").len(), 1);
        }
    }

    #[test]
    fn journal_tag_read_rejects_changed_tagger_and_object() {
        let name = "velnor-receipt/12/1/publication-started";
        let blob = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
        let sha = super::journal_tag_hash(name, blob).expect("sha");
        let valid = serde_json::json!({"sha":sha,"tag":name,"message":"velnor-release-receipt-v1","object":{"sha":blob,"type":"blob"},"tagger":{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"},"verification":{"verified":false,"reason":"unsigned","signature":null,"payload":null}});
        for field in ["tagger", "object", "verification"] {
            let mut bad = valid.clone();
            bad[field] = serde_json::json!({});
            let (origin, worker) = server(vec![reply("200 OK", &bad.to_string())]);
            assert!(
                client(origin)
                    .get_journal_tag("example/demo-crate", &sha)
                    .is_err()
            );
            worker.join().expect("server");
        }
    }

    #[test]
    fn journal_blob_write_requires_exact_content_identity() {
        let sha = "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0";
        let (origin, worker) = server(vec![reply("201 Created", &format!(r#"{{"sha":"{sha}"}}"#))]);
        assert_eq!(
            client(origin)
                .create_journal_blob("example/demo-crate", b"hello")
                .expect("valid blob"),
            sha
        );
        let requests = worker.join().expect("server");
        assert!(requests[0].contains("\"encoding\":\"base64\""));
        assert!(requests[0].contains("\"content\":\"aGVsbG8=\""));
        let (origin, worker) = server(vec![reply(
            "201 Created",
            &format!(r#"{{"sha":"{}"}}"#, "a".repeat(40)),
        )]);
        assert!(
            client(origin)
                .create_journal_blob("example/demo-crate", b"hello")
                .is_err()
        );
        worker.join().expect("server");
    }

    fn server(replies: Vec<String>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        listener.set_nonblocking(true).expect("nonblocking");
        let address = listener.local_addr().expect("address");
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(5);
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("expected request: {error}"),
                    }
                };
                stream.set_nonblocking(false).expect("blocking stream");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("read timeout");
                let mut request = Vec::new();
                let mut buffer = [0; 8192];
                loop {
                    let size = stream.read(&mut buffer).expect("read");
                    assert!(size > 0);
                    request.extend_from_slice(&buffer[..size]);
                    if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let headers =
                            String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("content-length: ")
                                    .and_then(|n| n.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= index + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                stream.write_all(reply.as_bytes()).expect("write");
            }
            requests
        });
        (format!("http://{address}"), worker)
    }

    fn reply(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn client(origin: String) -> GitHubAdmissionHttp {
        GitHubAdmissionHttp {
            token: TOKEN.to_owned(),
            deadline: Instant::now() + Duration::from_secs(5),
            origin,
        }
    }

    #[test]
    fn reads_authenticated_json_and_does_not_follow_redirects() {
        let (origin, worker) = server(vec![reply("200 OK", r#"{"ok":true}"#)]);
        let response = client(origin)
            .get("repos/example/demo-crate")
            .expect("authenticated JSON");
        assert_eq!(response["ok"], true);
        let requests = worker.join().expect("server");
        let request = requests[0].to_ascii_lowercase();
        assert!(request.contains("authorization: bearer synthetic-test-credential"));
        assert!(request.contains("accept: application/vnd.github+json"));
        assert!(request.contains("x-github-api-version: 2022-11-28"));
        let sink = TcpListener::bind("127.0.0.1:0").expect("redirect sink");
        sink.set_nonblocking(true).expect("nonblocking");
        let location = format!("http://{}/stolen", sink.local_addr().expect("address"));
        let (origin, worker) = server(vec![format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )]);
        assert!(client(origin).get("repos/example/demo-crate").is_err());
        worker.join().expect("server");
        assert_eq!(
            sink.accept()
                .expect_err("redirect must not be requested")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn accepts_encoded_branch_and_check_pagination() {
        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Encoding: identity\r\nConnection: close\r\n\r\n{}";
        let (origin, worker) = server(vec![response.to_owned(), reply("200 OK", "{}")]);
        let client = client(origin);
        assert!(
            client
                .get("repos/example/demo-crate/git/ref/heads/release%2Fmain")
                .is_ok()
        );
        assert!(client.get("repos/example/demo-crate/commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/check-runs?filter=all&per_page=100&page=2").is_ok());
        assert_eq!(worker.join().expect("server").len(), 2);
    }

    #[test]
    fn accepts_same_run_artifact_metadata_routes() {
        let (origin, worker) = server(vec![reply("200 OK", "{}"), reply("200 OK", "{}")]);
        let client = client(origin);
        assert!(
            client
                .get("repos/example/demo-crate/actions/runs/123/artifacts?per_page=100&page=1")
                .is_ok()
        );
        assert!(
            client
                .get("repos/example/demo-crate/actions/artifacts/456")
                .is_ok()
        );
        assert_eq!(worker.join().expect("server").len(), 2);
    }

    #[test]
    fn posts_exact_approved_tag_and_release() {
        let tag_response = r#"{"ref":"refs/tags/demo-crate-v0.1.1","object":{"type":"commit","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#;
        let release_response = r#"{"tag_name":"demo-crate-v0.1.1","target_commitish":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","draft":false,"html_url":"https://github.com/example/demo-crate/releases/tag/demo-crate-v0.1.1"}"#;
        let (origin, worker) = server(vec![
            reply("201 Created", tag_response),
            reply("201 Created", release_response),
        ]);
        let client = client(origin);
        assert!(
            client
                .create_tag(
                    "example/demo-crate",
                    "demo-crate-v0.1.1",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                )
                .is_ok()
        );
        assert!(
            client
                .create_release(
                    "example/demo-crate",
                    "demo-crate-v0.1.1",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                )
                .is_ok()
        );
        let requests = worker.join().expect("server");
        assert!(requests[0].starts_with("POST /repos/example/demo-crate/git/refs "));
        assert!(requests[1].starts_with("POST /repos/example/demo-crate/releases "));
        let tag_body: serde_json::Value =
            serde_json::from_str(requests[0].split_once("\r\n\r\n").expect("body").1)
                .expect("JSON body");
        let release_body: serde_json::Value =
            serde_json::from_str(requests[1].split_once("\r\n\r\n").expect("body").1)
                .expect("JSON body");
        assert_eq!(
            tag_body,
            serde_json::json!({"ref":"refs/tags/demo-crate-v0.1.1", "sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})
        );
        assert_eq!(
            release_body,
            serde_json::json!({"tag_name":"demo-crate-v0.1.1", "name":"demo-crate-v0.1.1", "target_commitish":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})
        );
    }

    #[test]
    fn rejects_invalid_writes_before_network() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        listener.set_nonblocking(true).expect("nonblocking");
        let client = client(format!(
            "http://{}",
            listener.local_addr().expect("address")
        ));
        for (repo, tag, sha) in [
            (
                "bad/repo/extra",
                "demo-crate-v0.1.1",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            (
                "example/demo-crate",
                "../escape",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            (
                "example/demo-crate",
                "v1.lock",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            ("example/demo-crate", "v1", "not-a-sha"),
        ] {
            let result = client.create_tag(repo, tag, sha);
            assert!(result.is_err());
            assert!(!format!("{result:?}").contains(TOKEN));
            assert!(client.create_release(repo, tag, sha).is_err());
        }
        assert_eq!(
            listener
                .accept()
                .expect_err("invalid write must not send")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn rejects_non_created_and_wrong_created_responses() {
        for status in [
            "200 OK",
            "202 Accepted",
            "301 Moved Permanently",
            "302 Found",
            "401 Unauthorized",
            "403 Forbidden",
            "409 Conflict",
            "422 Unprocessable Entity",
            "429 Too Many Requests",
            "500 Server Error",
        ] {
            for release in [false, true] {
                let (origin, worker) = server(vec![reply(status, "{}")]);
                let client = client(origin);
                let result = if release {
                    client.create_release(
                        "example/demo-crate",
                        "demo-crate-v0.1.1",
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    )
                } else {
                    client.create_tag(
                        "example/demo-crate",
                        "demo-crate-v0.1.1",
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    )
                };
                assert!(result.is_err(), "accepted {status}");
                assert!(!format!("{result:?}").contains(TOKEN));
                assert_eq!(worker.join().expect("server").len(), 1);
            }
        }
        for body in [
            "not-json",
            r#"{"ref":"refs/tags/demo-crate-v0.1.1","object":{"type":"commit","sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}"#,
            r#"{"ref":"refs/tags/other","object":{"type":"commit","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#,
        ] {
            let (origin, worker) = server(vec![reply("201 Created", body)]);
            assert!(
                client(origin)
                    .create_tag(
                        "example/demo-crate",
                        "demo-crate-v0.1.1",
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    )
                    .is_err()
            );
            worker.join().expect("server");
        }
        for body in [
            r#"{"tag_name":"other","draft":false,"html_url":"https://github.com/example/demo-crate/releases/tag/demo-crate-v0.1.1"}"#,
            r#"{"tag_name":"demo-crate-v0.1.1","draft":true,"html_url":"https://github.com/example/demo-crate/releases/tag/demo-crate-v0.1.1"}"#,
            r#"{"tag_name":"demo-crate-v0.1.1","draft":false,"html_url":"https://evil.test/"}"#,
        ] {
            let (origin, worker) = server(vec![reply("201 Created", body)]);
            assert!(
                client(origin)
                    .create_release(
                        "example/demo-crate",
                        "demo-crate-v0.1.1",
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    )
                    .is_err()
            );
            worker.join().expect("server");
        }
    }

    #[test]
    fn ambiguous_post_failure_is_not_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        let address = listener.local_addr().expect("address");
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("timeout");
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let size = stream.read(&mut buffer).expect("read");
                assert!(size > 0);
                request.extend_from_slice(&buffer[..size]);
                if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..index]).to_ascii_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length: ")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .expect("content length");
                    if request.len() >= index + 4 + length {
                        break;
                    }
                }
            }
            drop(stream);
            listener.set_nonblocking(true).expect("nonblocking");
            thread::sleep(Duration::from_millis(500));
            assert_eq!(
                listener
                    .accept()
                    .expect_err("POST must not be retried")
                    .kind(),
                std::io::ErrorKind::WouldBlock
            );
        });
        let result = client(format!("http://{address}")).create_tag(
            "example/demo-crate",
            "demo-crate-v0.1.1",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        assert!(result.is_err());
        assert!(!format!("{result:?}").contains(TOKEN));
        worker.join().expect("server");
    }

    #[test]
    fn post_redirect_does_not_forward_credentials() {
        let sink = TcpListener::bind("127.0.0.1:0").expect("redirect sink");
        sink.set_nonblocking(true).expect("nonblocking");
        let location = format!("http://{}/stolen", sink.local_addr().expect("address"));
        let (origin, worker) = server(vec![format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )]);
        assert!(
            client(origin)
                .create_tag(
                    "example/demo-crate",
                    "demo-crate-v0.1.1",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                )
                .is_err()
        );
        worker.join().expect("server");
        assert_eq!(
            sink.accept()
                .expect_err("redirect must not be requested")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn post_rejects_ambiguous_headers_and_truncated_response() {
        for response in [
            "HTTP/1.1 201 Created\r\nContent-Length: 2\r\nContent-Length: 3\r\nConnection: close\r\n\r\n{}".to_owned(),
            "HTTP/1.1 201 Created\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}".to_owned(),
            "HTTP/1.1 201 Created\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n".to_owned(),
        ] {
            let (origin, worker) = server(vec![response]);
            assert!(client(origin).create_tag("example/demo-crate", "demo-crate-v0.1.1", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_err());
            worker.join().expect("server");
        }
    }

    #[test]
    fn post_deadline_bounds_stalled_response() {
        crate::api::github_evidence_http::agent().expect("TLS setup");
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        let address = listener.local_addr().expect("address");
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0; 4096];
            assert!(stream.read(&mut buffer).expect("read") > 0);
            stream
                .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 20\r\n\r\n{")
                .expect("headers");
            thread::sleep(Duration::from_secs(3));
        });
        let client = GitHubAdmissionHttp {
            token: TOKEN.to_owned(),
            deadline: Instant::now() + Duration::from_secs(2),
            origin: format!("http://{address}"),
        };
        assert!(
            client
                .create_tag(
                    "example/demo-crate",
                    "demo-crate-v0.1.1",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                )
                .is_err()
        );
        worker.join().expect("server");
    }

    #[test]
    fn rejects_unsafe_routes_without_sending_credentials() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        listener.set_nonblocking(true).expect("nonblocking");
        let client = client(format!(
            "http://{}",
            listener.local_addr().expect("address")
        ));
        for route in [
            "https://evil.test/",
            "repos/example/demo-crate/%2E%2E/secrets",
            "repos/example/demo-crate/git/ref/tags/%252e%252e",
            "repos/example/demo-crate/git/ref/tags/a%0D%0AInjected",
            "repos/example/demo-crate?x=1",
            "repos/example/demo-crate/git/ref/tags/a%2F..%2Fsecret",
            "users/name@evil.test",
        ] {
            let result = client.get(route);
            assert!(result.is_err(), "accepted {route}");
            assert!(!format!("{result:?}").contains(TOKEN));
        }
        assert_eq!(
            listener
                .accept()
                .expect_err("invalid routes must not send")
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(!format!("{client:?}").contains(TOKEN));
    }

    #[test]
    fn rejects_errors_malformed_and_oversized_bodies() {
        for status in [
            "401 Unauthorized",
            "403 Forbidden",
            "404 Not Found",
            "429 Too Many Requests",
            "500 Server Error",
        ] {
            let (origin, worker) = server(vec![reply(status, "{}")]);
            let result = client(origin).get("repos/example/demo-crate");
            assert!(result.is_err(), "accepted {status}");
            assert!(!format!("{result:?}").contains(TOKEN));
            worker.join().expect("server");
        }
        for response in [reply("200 OK", "not-json"), "HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n".to_owned(), "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n{}".to_owned(), "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\n{}".to_owned()] {
            let (origin, worker) = server(vec![response]);
            assert!(client(origin).get("repos/example/demo-crate").is_err());
            worker.join().expect("server");
        }
    }

    #[test]
    fn bounds_streamed_json_without_content_length() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        let address = listener.local_addr().expect("address");
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0; 8192];
            assert!(stream.read(&mut buffer).expect("read") > 0);
            let body = format!("\"{}\"", "x".repeat(8 * 1024 * 1024));
            let headers = format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n",
                body.len()
            );
            let _written = stream
                .write_all(headers.as_bytes())
                .and_then(|()| stream.write_all(body.as_bytes()))
                .and_then(|()| stream.write_all(b"\r\n0\r\n\r\n"));
        });
        assert!(
            client(format!("http://{address}"))
                .get("repos/example/demo-crate")
                .is_err()
        );
        worker.join().expect("server");
    }

    #[test]
    fn optional_absence_requires_access_to_default_branch() {
        let repo = r#"{"full_name":"example/demo-crate","default_branch":"main"}"#;
        let branch = r#"{"ref":"refs/heads/main","object":{"type":"commit","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#;
        let (origin, worker) = server(vec![
            reply("404 Not Found", "{}"),
            reply("200 OK", repo),
            reply("200 OK", branch),
        ]);
        assert!(
            client(origin)
                .get_optional("repos/example/demo-crate/git/ref/tags/demo-crate-v0.1.1")
                .expect("authorized absence")
                .is_none()
        );
        assert_eq!(worker.join().expect("server").len(), 3);
        let (origin, worker) = server(vec![
            reply("404 Not Found", "{}"),
            reply("403 Forbidden", "{}"),
        ]);
        assert!(
            client(origin)
                .get_optional("repos/example/demo-crate/git/ref/tags/demo-crate-v0.1.1")
                .is_err()
        );
        worker.join().expect("server");
        let (origin, worker) = server(vec![
            reply("404 Not Found", "{}"),
            reply("200 OK", repo),
            reply("404 Not Found", "{}"),
        ]);
        assert!(
            client(origin)
                .get_optional("repos/example/demo-crate/releases/tags/demo-crate-v0.1.1")
                .is_err()
        );
        worker.join().expect("server");
    }

    #[test]
    fn deadline_stops_a_stalled_response_body() {
        crate::api::github_evidence_http::agent().expect("TLS setup");
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        let address = listener.local_addr().expect("address");
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0; 4096];
            assert!(stream.read(&mut buffer).expect("read") > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\n\r\n{")
                .expect("headers");
            thread::sleep(Duration::from_secs(3));
        });
        let client = GitHubAdmissionHttp {
            token: TOKEN.to_owned(),
            deadline: Instant::now() + Duration::from_secs(2),
            origin: format!("http://{address}"),
        };
        assert!(client.get("repos/example/demo-crate").is_err());
        worker.join().expect("server");
    }
}
