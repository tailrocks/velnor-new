// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use reqwest::{Client, RequestBuilder, Response, StatusCode, Url};
use serde_json::Value;

use crate::error::HostError;

use super::manifest::REPOSITORY;
use super::strict_json;

#[path = "github_response.rs"]
mod response;

const API_ROOT: &str = "https://api.github.com/";
const API_VERSION: &str = "2022-11-28";
const MAX_RELEASE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ATTESTATION_BYTES: usize = 16 * 1024 * 1024;
const ASSET_REDIRECT_HOST: &str = "release-assets.githubusercontent.com";
const USER_AGENT: &str = "velnor-runner-native-artifact-admission";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Asset {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) digest: String,
}

pub(super) fn client() -> Result<Client, HostError> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|_| HostError::Identity)
}

pub(super) fn valid_pat(pat: &str) -> bool {
    !pat.is_empty() && pat.len() <= 4096 && !pat.chars().any(char::is_control)
}

pub(super) async fn runner_release(
    client: &Client,
    pat: &str,
    source: &str,
) -> Result<BTreeMap<String, Asset>, HostError> {
    let path = format!("repos/{REPOSITORY}/releases/tags/runner-{source}");
    let value = get_api_json(client, pat, &path, MAX_RELEASE_BYTES).await?;
    let assets = response::parse_runner_release(source, value)?;
    let reference = format!("repos/{REPOSITORY}/git/ref/tags/runner-{source}");
    let value = get_api_json(client, pat, &reference, MAX_RELEASE_BYTES).await?;
    let object = value
        .get("object")
        .and_then(Value::as_object)
        .ok_or(HostError::Identity)?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(HostError::Identity)?;
    let sha = object
        .get("sha")
        .and_then(Value::as_str)
        .ok_or(HostError::Identity)?;
    let target = if kind == "tag" {
        if !super::hash::is_lower_hex(sha, 40) {
            return Err(HostError::Identity);
        }
        let path = format!("repos/{REPOSITORY}/git/tags/{sha}");
        let tag = get_api_json(client, pat, &path, MAX_RELEASE_BYTES).await?;
        let object = tag
            .get("object")
            .and_then(Value::as_object)
            .ok_or(HostError::Identity)?;
        if object.get("type").and_then(Value::as_str) != Some("commit") {
            return Err(HostError::Identity);
        }
        object
            .get("sha")
            .and_then(Value::as_str)
            .ok_or(HostError::Identity)?
            .to_owned()
    } else if kind == "commit" {
        sha.to_owned()
    } else {
        return Err(HostError::Identity);
    };
    if target != source {
        return Err(HostError::Identity);
    }
    Ok(assets)
}

pub(super) async fn download_asset(
    client: &Client,
    pat: &str,
    asset: &Asset,
    max_bytes: usize,
) -> Result<Vec<u8>, HostError> {
    let url = format!("{API_ROOT}repos/{REPOSITORY}/releases/assets/{}", asset.id);
    let response = client
        .get(url)
        .header(ACCEPT, "application/octet-stream")
        .bearer_auth(pat)
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    match response.status() {
        StatusCode::OK => read_limited(response, max_bytes).await,
        StatusCode::FOUND => download_redirect(client, response, max_bytes).await,
        _ => Err(HostError::Identity),
    }
}

async fn download_redirect(
    client: &Client,
    response: Response,
    max_bytes: usize,
) -> Result<Vec<u8>, HostError> {
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(HostError::Identity)?;
    let url = validate_asset_redirect(location)?;
    let response = asset_redirect_request(client, url)
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    if response.status() != StatusCode::OK {
        return Err(HostError::Identity);
    }
    read_limited(response, max_bytes).await
}

fn asset_redirect_request(client: &Client, url: Url) -> reqwest::RequestBuilder {
    client.get(url).header(ACCEPT, "application/octet-stream")
}

pub(super) fn validate_asset_redirect(location: &str) -> Result<Url, HostError> {
    let url = Url::parse(location).map_err(|_| HostError::Identity)?;
    if url.scheme() != "https"
        || url.host_str() != Some(ASSET_REDIRECT_HOST)
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() == "/"
        || url.as_str().bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(HostError::Identity);
    }
    Ok(url)
}

pub(super) async fn attestations(
    client: &Client,
    pat: &str,
    checksum_digest: &str,
) -> Result<Vec<Value>, HostError> {
    let route = response::attestation_path(checksum_digest)?;
    let mut bundles = Vec::new();
    let mut total_bytes = 0_usize;
    let mut path = format!("{route}?per_page=100");
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..20_usize {
        if !visited.insert(path.clone()) {
            return Err(HostError::Identity);
        }
        let response = api_request(client, pat, &path)?
            .send()
            .await
            .map_err(|_| HostError::Identity)?;
        if response.status() != StatusCode::OK {
            return Err(HostError::Identity);
        }
        let next = next_page(response.headers(), &route)?;
        let bytes = read_limited(response, MAX_ATTESTATION_BYTES).await?;
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_ATTESTATION_BYTES)
            .ok_or(HostError::Frame)?;
        let value = strict_json::parse(&bytes)?;
        let rows = response::parse_attestations(&value)?;
        for mut row in rows {
            response::validate_attestation_repository(&row)?;
            if row.get("bundle").is_none_or(Value::is_null) {
                let url = row
                    .get("bundle_url")
                    .and_then(Value::as_str)
                    .ok_or(HostError::Identity)?;
                let bundle = fetch_bundle(client, url).await?;
                let size = serde_json::to_vec(&bundle)
                    .map_err(|_| HostError::Identity)?
                    .len();
                total_bytes = total_bytes
                    .checked_add(size)
                    .filter(|size| *size <= MAX_ATTESTATION_BYTES)
                    .ok_or(HostError::Frame)?;
                row.as_object_mut()
                    .ok_or(HostError::Identity)?
                    .insert("bundle".to_owned(), bundle);
            }
            bundles.push(response::parse_attestation(&row)?);
        }
        match next {
            Some(next) => path = next,
            None => return Ok(bundles),
        }
    }
    Err(HostError::Identity)
}

async fn fetch_bundle(client: &Client, value: &str) -> Result<Value, HostError> {
    let url = Url::parse(value).map_err(|_| HostError::Identity)?;
    let host = url.host_str().ok_or(HostError::Identity)?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !(host.ends_with(".blob.core.windows.net")
            || host.ends_with(".actions.githubusercontent.com"))
    {
        return Err(HostError::Identity);
    }
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    if response.status() != StatusCode::OK
        || response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .is_some_and(|value| value.as_bytes() != b"identity")
    {
        return Err(HostError::Identity);
    }
    let compressed = read_limited(response, MAX_ATTESTATION_BYTES).await?;
    let size = snap::raw::decompress_len(&compressed).map_err(|_| HostError::Identity)?;
    if size == 0 || size > MAX_ATTESTATION_BYTES {
        return Err(HostError::Frame);
    }
    let bytes = snap::raw::Decoder::new()
        .decompress_vec(&compressed)
        .map_err(|_| HostError::Identity)?;
    strict_json::parse(&bytes)
}

fn next_page(headers: &HeaderMap, route: &str) -> Result<Option<String>, HostError> {
    let mut next = None;
    for header in headers.get_all(reqwest::header::LINK) {
        let link = header.to_str().map_err(|_| HostError::Identity)?;
        for part in link.split(',') {
            let (url, params) = part.trim().split_once(';').ok_or(HostError::Identity)?;
            let url = url
                .trim()
                .strip_prefix('<')
                .and_then(|url| url.strip_suffix('>'))
                .ok_or(HostError::Identity)?;
            let relations = params
                .split(';')
                .map(str::trim)
                .filter_map(|param| param.strip_prefix("rel="))
                .collect::<Vec<_>>();
            if relations.len() != 1 {
                return Err(HostError::Identity);
            }
            if relations[0] != "\"next\"" {
                continue;
            }
            if next.is_some() {
                return Err(HostError::Identity);
            }
            let url = Url::parse(url).map_err(|_| HostError::Identity)?;
            let subject = route
                .strip_prefix("repos/tailrocks/velnor-new/attestations/")
                .ok_or(HostError::Identity)?;
            let expected_path = format!("/repositories/1390620900/attestations/{subject}");
            let original_path = format!("/{route}");
            let query = url.query_pairs().collect::<Vec<_>>();
            if url.scheme() != "https"
                || url.host_str() != Some("api.github.com")
                || url.port_or_known_default() != Some(443)
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
                || (url.path() != expected_path && url.path() != original_path)
                || query.len() != 2
                || !query
                    .iter()
                    .any(|(key, value)| key == "per_page" && value == "100")
                || !query.iter().any(|(key, value)| {
                    key == "after"
                        && !value.is_empty()
                        && value.len() <= 512
                        && !value.chars().any(char::is_control)
                })
            {
                return Err(HostError::Identity);
            }
            let path = url
                .as_str()
                .strip_prefix(API_ROOT)
                .ok_or(HostError::Identity)?
                .to_owned();
            next = Some(path);
        }
    }
    Ok(next)
}

async fn get_api_json(
    client: &Client,
    pat: &str,
    path: &str,
    limit: usize,
) -> Result<Value, HostError> {
    let response = api_request(client, pat, path)?
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    if response.status() != StatusCode::OK {
        return Err(HostError::Identity);
    }
    let bytes = read_limited(response, limit).await?;
    strict_json::parse(&bytes)
}

fn api_url(path: &str) -> Result<Url, HostError> {
    let url = Url::parse(API_ROOT)
        .and_then(|base| base.join(path))
        .map_err(|_| HostError::Identity)?;
    if url.scheme() != "https" || url.host_str() != Some("api.github.com") {
        return Err(HostError::Identity);
    }
    Ok(url)
}

fn api_request(client: &Client, pat: &str, path: &str) -> Result<RequestBuilder, HostError> {
    let url = api_url(path)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static(API_VERSION),
    );
    Ok(client.get(url).headers(headers).bearer_auth(pat))
}

pub(super) async fn read_limited(response: Response, limit: usize) -> Result<Vec<u8>, HostError> {
    let content_limit = u64::try_from(limit).map_err(|_| HostError::Frame)?;
    if response
        .content_length()
        .is_some_and(|length| length > content_limit)
    {
        return Err(HostError::Frame);
    }
    let mut output = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| HostError::Identity)?;
        output
            .len()
            .checked_add(chunk.len())
            .filter(|size| *size <= limit)
            .ok_or(HostError::Frame)?;
        output
            .try_reserve(chunk.len())
            .map_err(|_| HostError::Frame)?;
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}
