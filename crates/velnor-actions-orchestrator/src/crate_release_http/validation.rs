// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Instant, OrchestratorError, Read, Value, internal, valid_headers, valid_sha};

pub(super) fn parse_json(
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

pub(super) fn no_store(headers: &ureq::http::HeaderMap) -> bool {
    headers.get_all("cache-control").iter().any(|value| {
        value.to_str().map_or(true, |text| {
            text.split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("no-store"))
        })
    })
}

pub(super) fn response_etag(headers: &ureq::http::HeaderMap) -> Option<String> {
    if no_store(headers) || headers.get_all("etag").iter().count() != 1 {
        return None;
    }
    let value = headers.get("etag")?.to_str().ok()?;
    let opaque = value.strip_prefix("W/").unwrap_or(value);
    let bytes = opaque.as_bytes();
    if value.len() > 256
        || bytes.len() < 2
        || bytes.first() != Some(&b'"')
        || bytes.last() != Some(&b'"')
        || bytes[1..bytes.len() - 1]
            .iter()
            .any(|b| !(*b == 0x21 || (0x23..=0x7e).contains(b)))
    {
        return None;
    }
    Some(value.to_owned())
}

pub(super) fn validate_journal_name(name: &str) -> Result<(), OrchestratorError> {
    let parts: Vec<_> = name.split('/').collect();
    if !matches!(parts.as_slice(), ["velnor-receipt", run, attempt, phase] if canonical_positive(run) && canonical_positive(attempt) && crate::crate_release_journal::valid_journal_phase(phase))
    {
        return Err(internal("crate_release_http_write_input"));
    }
    Ok(())
}

pub(super) fn canonical_positive(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|number| number > 0 && number.to_string() == value)
}

pub(super) fn journal_tag_hash(tag: &str, blob_sha: &str) -> Result<String, OrchestratorError> {
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

pub(super) fn validate_journal_tag_response(
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

pub(super) fn validate_repository(repository: &str) -> Result<(), OrchestratorError> {
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

pub(super) fn validate_write(
    repository: &str,
    tag: &str,
    sha: &str,
) -> Result<(), OrchestratorError> {
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
