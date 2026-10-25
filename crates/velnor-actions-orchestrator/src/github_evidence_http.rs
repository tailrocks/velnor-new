use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use sha2::{Digest, Sha256};

#[path = "github_evidence_http_route.rs"]
mod route;
pub(super) use route::EvidenceRoute;

use crate::OrchestratorError;
use crate::internal::internal;

const API_ORIGIN: &str = "https://api.github.com";
const JSON_LIMIT: u64 = 8 * 1024 * 1024;
const REDIRECT_LIMIT: usize = 3;
const IO_CHUNK: usize = 64 * 1024;
static AGENT: OnceLock<Result<ureq::Agent, String>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GitHubRepository {
    pub(super) owner: String,
    pub(super) name: String,
}

impl GitHubRepository {
    pub(super) fn parse(slug: &str) -> Result<Self, OrchestratorError> {
        let Some((owner, name)) = slug.split_once('/') else {
            return Err(internal("github_evidence_repository_invalid"));
        };
        if slug.matches('/').count() != 1
            || !velnor_actions_contract::config::is_valid_github_owner(owner)
            || !velnor_actions_contract::config::is_valid_github_repo_name(name)
        {
            return Err(internal("github_evidence_repository_invalid"));
        }
        Ok(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }

    fn route_prefix(&self) -> String {
        format!("/repos/{}/{}", self.owner, self.name)
    }
}

pub(super) struct DownloadReceipt {
    pub(super) size_bytes: u64,
    pub(super) sha256: String,
}

pub(super) struct ArtifactArchiveRequest<'a> {
    pub(super) repo: &'a GitHubRepository,
    pub(super) artifact_id: u64,
    pub(super) token: &'a str,
    pub(super) expected_size: u64,
    pub(super) expected_sha256: &'a str,
    pub(super) dest: &'a Path,
    pub(super) deadline: Instant,
}

pub(super) trait GitHubEvidenceRead {
    fn json(
        &self,
        repo: &GitHubRepository,
        route: &EvidenceRoute,
        token: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, OrchestratorError>;

    fn archive(
        &self,
        request: ArtifactArchiveRequest<'_>,
    ) -> Result<DownloadReceipt, OrchestratorError>;
}

pub(super) struct NativeGitHubEvidenceRead;

impl GitHubEvidenceRead for NativeGitHubEvidenceRead {
    fn json(
        &self,
        repo: &GitHubRepository,
        route: &EvidenceRoute,
        token: &str,
        deadline: Instant,
    ) -> Result<Vec<u8>, OrchestratorError> {
        let url = format!("{API_ORIGIN}{}", route.path(repo));
        let response = api_request(&url, token, deadline)?;
        read_json_response(response, deadline)
    }

    fn archive(
        &self,
        request: ArtifactArchiveRequest<'_>,
    ) -> Result<DownloadReceipt, OrchestratorError> {
        let ArtifactArchiveRequest {
            repo,
            artifact_id,
            token,
            expected_size,
            expected_sha256,
            dest,
            deadline,
        } = request;
        if expected_size == 0
            || expected_size > velnor_actions_contract::MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES
            || !velnor_actions_contract::is_valid_digest(expected_sha256)
        {
            return Err(internal("github_evidence_archive_bound"));
        }
        let url = format!(
            "{API_ORIGIN}{}",
            EvidenceRoute::ArtifactDownload { artifact_id }.path(repo)
        );
        let response = api_request(&url, token, deadline)?;
        if response.status().as_u16() != 302 {
            return Err(internal("github_evidence_archive_redirect"));
        }
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| internal("github_evidence_archive_location"))?;
        let mut current = checked_storage_uri(location)?;
        for redirects in 0..=REDIRECT_LIMIT {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(internal("github_evidence_archive_deadline"));
            }
            let mut request = agent()?.get(&current);
            for (name, value) in storage_request_headers() {
                request = request.header(name, &value);
            }
            let mut response = request
                .config()
                .timeout_global(Some(remaining))
                .max_redirects(0)
                .build()
                .call()
                .map_err(|_| internal("github_evidence_archive_transport"))?;
            if response.status().is_redirection() {
                if redirects == REDIRECT_LIMIT {
                    return Err(internal("github_evidence_archive_redirect_limit"));
                }
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| internal("github_evidence_archive_location"))?;
                current = next_storage_uri(location, redirects)?;
                continue;
            }
            if response.status().as_u16() != 200
                || !valid_archive_response_headers(response.headers(), expected_size)
            {
                return Err(internal("github_evidence_archive_response"));
            }
            let result = stream_archive(
                &mut response,
                dest,
                expected_size,
                expected_sha256,
                deadline,
            );
            if result.is_err() {
                drop(fs::remove_file(dest));
            }
            return result;
        }
        Err(internal("github_evidence_archive_redirect_limit"))
    }
}

fn api_request(
    url: &str,
    token: &str,
    deadline: Instant,
) -> Result<ureq::http::Response<ureq::Body>, OrchestratorError> {
    if token.is_empty() {
        return Err(internal("github_evidence_token_missing"));
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(internal("github_evidence_api_deadline"));
    }
    let mut request = agent()?.get(url);
    for (name, value) in api_request_headers(token) {
        request = request.header(name, &value);
    }
    request
        .config()
        .timeout_global(Some(remaining))
        .max_redirects(0)
        .build()
        .call()
        .map_err(|_| internal("github_evidence_api_transport"))
}

fn api_request_headers(token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Authorization", format!("Bearer {token}")),
        ("Accept", "application/vnd.github+json".to_owned()),
        ("X-GitHub-Api-Version", "2022-11-28".to_owned()),
        ("Accept-Encoding", "identity".to_owned()),
        ("User-Agent", "velnor-actions-github-evidence".to_owned()),
    ]
}

fn storage_request_headers() -> Vec<(&'static str, String)> {
    vec![
        ("Accept-Encoding", "identity".to_owned()),
        ("User-Agent", "velnor-actions-github-evidence".to_owned()),
    ]
}

fn read_json_response(
    response: ureq::http::Response<ureq::Body>,
    deadline: Instant,
) -> Result<Vec<u8>, OrchestratorError> {
    if response.status().as_u16() != 200 {
        return Err(internal("github_evidence_api_status"));
    }
    let mut body = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(JSON_LIMIT.saturating_add(1))
        .read_to_end(&mut body)
        .map_err(|_| internal("github_evidence_api_read"))?;
    if body.is_empty() || body.len() as u64 > JSON_LIMIT || Instant::now() >= deadline {
        return Err(internal("github_evidence_api_body_limit"));
    }
    Ok(body)
}

fn stream_archive(
    response: &mut ureq::http::Response<ureq::Body>,
    dest: &Path,
    expected_size: u64,
    expected_sha256: &str,
    deadline: Instant,
) -> Result<DownloadReceipt, OrchestratorError> {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|_| internal("github_evidence_archive_destination"))?;
    let mut reader = response.body_mut().as_reader();
    let mut buffer = vec![0_u8; IO_CHUNK];
    let mut size = 0_u64;
    let mut hash = Sha256::new();
    loop {
        if Instant::now() >= deadline {
            return Err(internal("github_evidence_archive_deadline"));
        }
        let count = reader
            .read(&mut buffer)
            .map_err(|_| internal("github_evidence_archive_read"))?;
        if count == 0 {
            break;
        }
        size = size.saturating_add(count as u64);
        if size > expected_size {
            return Err(internal("github_evidence_archive_size_mismatch"));
        }
        hash.update(&buffer[..count]);
        file.write_all(&buffer[..count])
            .map_err(|_| internal("github_evidence_archive_write"))?;
    }
    if size != expected_size {
        return Err(internal("github_evidence_archive_size_mismatch"));
    }
    let digest = hex_lower(&hash.finalize());
    if digest != expected_sha256 {
        return Err(internal("github_evidence_archive_digest_mismatch"));
    }
    file.sync_all()
        .map_err(|_| internal("github_evidence_archive_write"))?;
    Ok(DownloadReceipt {
        size_bytes: size,
        sha256: digest,
    })
}

fn identity_encoding(headers: &ureq::http::HeaderMap) -> bool {
    let mut values = headers.get_all("content-encoding").iter();
    match (values.next(), values.next()) {
        (None, None) => true,
        (Some(value), None) => value
            .to_str()
            .is_ok_and(|value| value.eq_ignore_ascii_case("identity")),
        _ => false,
    }
}

fn valid_archive_response_headers(headers: &ureq::http::HeaderMap, expected_size: u64) -> bool {
    if !identity_encoding(headers) {
        return false;
    }
    let mut lengths = headers.get_all("content-length").iter();
    match (lengths.next(), lengths.next()) {
        (None, None) => true,
        (Some(value), None) => value
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|size| size == expected_size),
        _ => false,
    }
}

fn next_storage_uri(location: &str, redirects: usize) -> Result<String, OrchestratorError> {
    if redirects >= REDIRECT_LIMIT {
        return Err(internal("github_evidence_archive_redirect_limit"));
    }
    checked_storage_uri(location)
}

fn checked_storage_uri(value: &str) -> Result<String, OrchestratorError> {
    let uri = value
        .parse::<ureq::http::Uri>()
        .map_err(|_| internal("github_evidence_archive_location"))?;
    let Some(authority) = uri.authority() else {
        return Err(internal("github_evidence_archive_location"));
    };
    let host = authority.host().to_ascii_lowercase();
    if uri.scheme_str() != Some("https")
        || authority.as_str().contains('@')
        || authority.port_u16().is_some_and(|port| port != 443)
        || !(host.ends_with(".blob.core.windows.net")
            || host.ends_with(".actions.githubusercontent.com"))
    {
        return Err(internal("github_evidence_archive_location"));
    }
    Ok(uri.to_string())
}

fn agent() -> Result<&'static ureq::Agent, OrchestratorError> {
    AGENT
        .get_or_init(build_agent)
        .as_ref()
        .map_err(|_| internal("github_evidence_tls_roots"))
}

fn build_agent() -> Result<ureq::Agent, String> {
    let trust = rustls_native_certs::load_native_certs();
    if !trust.errors.is_empty() || trust.certs.is_empty() {
        return Err("platform trust certificates unavailable".to_owned());
    }
    let roots = trust
        .certs
        .iter()
        .map(|certificate| ureq::tls::Certificate::from_der(certificate.as_ref()).to_owned())
        .collect::<Vec<_>>();
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(roots.into())
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let config = agent_config(tls);
    Ok(ureq::Agent::new_with_config(config))
}

fn agent_config(tls: ureq::tls::TlsConfig) -> ureq::config::Config {
    ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(None)
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .max_redirects(0)
        .build()
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(hex_digit(byte >> 4));
        result.push(hex_digit(byte & 0x0f));
    }
    result
}

fn hex_digit(value: u8) -> char {
    char::from(b"0123456789abcdef"[usize::from(value)])
}
