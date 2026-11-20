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
const ASSET_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
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
    let response = asset_response(client, pat, asset).await?;
    read_limited(response, max_bytes).await
}

async fn asset_response(client: &Client, pat: &str, asset: &Asset) -> Result<Response, HostError> {
    let url = format!("{API_ROOT}repos/{REPOSITORY}/releases/assets/{}", asset.id);
    let response = client
        .get(url)
        .header(ACCEPT, "application/octet-stream")
        .bearer_auth(pat)
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    match response.status() {
        StatusCode::OK => Ok(response),
        StatusCode::FOUND => download_redirect(client, response).await,
        _ => Err(HostError::Identity),
    }
}

async fn download_redirect(client: &Client, response: Response) -> Result<Response, HostError> {
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
    Ok(response)
}

fn asset_redirect_request(client: &Client, url: Url) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header(ACCEPT, "application/octet-stream")
        .timeout(ASSET_DOWNLOAD_TIMEOUT)
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
    let path = response::attestation_path(checksum_digest)?;
    let response = get_api_json(client, pat, &path, MAX_ATTESTATION_BYTES).await?;
    response::parse_attestations(&response)
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
