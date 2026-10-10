//! Fixed-origin GitHub API access and unauthenticated release-asset redirect handling.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use reqwest::{Client, Response, StatusCode, Url};
use serde::Deserialize;
use serde_json::Value;

use crate::error::HostError;

use super::hash::is_sha256_digest;
use super::manifest::{ARCHIVE_ASSET, CHECKSUM_ASSET, MANIFEST_ASSET, REPOSITORY};
use super::strict_json;

const API_ROOT: &str = "https://api.github.com/";
const API_VERSION: &str = "2022-11-28";
const PRODUCT_REPOSITORY_ID: u64 = 1_390_620_900;
const MAX_RELEASE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ATTESTATION_BYTES: usize = 16 * 1024 * 1024;
const MAX_ATTESTATIONS: usize = 16;
const MAX_NON_PROBE_ASSET_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const ASSET_REDIRECT_HOST: &str = "release-assets.githubusercontent.com";
const USER_AGENT: &str = "velnor-runner-native-artifact-admission";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Asset {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) digest: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    target_commitish: String,
    draft: bool,
    prerelease: bool,
    immutable: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    id: u64,
    name: String,
    state: String,
    size: u64,
    digest: String,
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

pub(super) async fn release(
    client: &Client,
    pat: &str,
    source: &str,
) -> Result<BTreeMap<String, Asset>, HostError> {
    let tag = format!("runner-{source}");
    let path = format!("repos/{REPOSITORY}/releases/tags/{tag}");
    let value = get_api_json(client, pat, &path, MAX_RELEASE_BYTES).await?;
    let release: Release = serde_json::from_value(value).map_err(|_| HostError::Identity)?;
    if release.tag_name != tag
        || release.target_commitish != source
        || release.draft
        || release.prerelease
        || !release.immutable
    {
        return Err(HostError::Identity);
    }
    let mut assets = BTreeMap::new();
    for asset in release.assets {
        if asset.id == 0
            || asset.state != "uploaded"
            || asset.size == 0
            || asset.size > MAX_NON_PROBE_ASSET_BYTES
            || !is_sha256_digest(&asset.digest)
            || assets
                .insert(
                    asset.name.clone(),
                    Asset {
                        id: asset.id,
                        name: asset.name,
                        size: asset.size,
                        digest: asset.digest,
                    },
                )
                .is_some()
        {
            return Err(HostError::Identity);
        }
    }
    let expected = BTreeSet::from([
        "velnor-runner-linux-amd64.tar",
        "velnor-dind-linux-amd64.tar",
        ARCHIVE_ASSET,
        MANIFEST_ASSET,
        CHECKSUM_ASSET,
    ]);
    if assets.len() != expected.len() || assets.keys().any(|name| !expected.contains(name.as_str()))
    {
        return Err(HostError::Identity);
    }
    let archive = assets.get(ARCHIVE_ASSET).ok_or(HostError::Identity)?;
    let manifest = assets.get(MANIFEST_ASSET).ok_or(HostError::Identity)?;
    let checksum = assets.get(CHECKSUM_ASSET).ok_or(HostError::Identity)?;
    if archive.size > 16 * 1024 * 1024 || manifest.size > 64 * 1024 || checksum.size > 64 * 1024 {
        return Err(HostError::Frame);
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
    let digest = checksum_digest
        .strip_prefix("sha256:")
        .ok_or(HostError::Identity)?;
    if !crate::artifact_admission::hash::is_lower_hex(digest, 64) {
        return Err(HostError::Identity);
    }
    let path = format!("repos/{REPOSITORY}/attestations/sha256:{digest}");
    let response = get_api_json(client, pat, &path, MAX_ATTESTATION_BYTES).await?;
    let rows = response
        .get("attestations")
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty() && rows.len() <= MAX_ATTESTATIONS)
        .ok_or(HostError::Identity)?;
    let mut bundles = Vec::new();
    for row in rows {
        if row.get("repository_id").and_then(Value::as_u64) != Some(PRODUCT_REPOSITORY_ID) {
            return Err(HostError::Identity);
        }
        if row
            .get("repository")
            .and_then(Value::as_str)
            .is_some_and(|repo| repo != REPOSITORY)
        {
            return Err(HostError::Identity);
        }
        let bundle = row
            .get("bundle")
            .filter(|bundle| bundle.is_object())
            .ok_or(HostError::Identity)?;
        if bundle.get("mediaType").and_then(Value::as_str)
            != Some("application/vnd.dev.sigstore.bundle.v0.3+json")
            || !bundle.get("dsseEnvelope").is_some_and(Value::is_object)
            || !bundle
                .get("verificationMaterial")
                .is_some_and(Value::is_object)
        {
            return Err(HostError::Identity);
        }
        bundles.push(bundle.clone());
    }
    if bundles.is_empty() {
        return Err(HostError::Identity);
    }
    Ok(bundles)
}

async fn get_api_json(
    client: &Client,
    pat: &str,
    path: &str,
    limit: usize,
) -> Result<Value, HostError> {
    let url = Url::parse(API_ROOT)
        .and_then(|base| base.join(path))
        .map_err(|_| HostError::Identity)?;
    if url.scheme() != "https" || url.host_str() != Some("api.github.com") {
        return Err(HostError::Identity);
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static(API_VERSION),
    );
    let response = client
        .get(url)
        .headers(headers)
        .bearer_auth(pat)
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    if response.status() != StatusCode::OK {
        return Err(HostError::Identity);
    }
    let bytes = read_limited(response, limit).await?;
    strict_json::parse(&bytes)
}

async fn read_limited(response: Response, limit: usize) -> Result<Vec<u8>, HostError> {
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

#[cfg(test)]
mod tests {
    use reqwest::header::AUTHORIZATION;

    use super::{asset_redirect_request, client, validate_asset_redirect};
    use crate::error::HostError;

    #[test]
    fn permits_only_the_fixed_https_asset_origin() -> Result<(), HostError> {
        let url = validate_asset_redirect(
            "https://release-assets.githubusercontent.com/releases/download/x?token=secret",
        )?;
        if url.host_str() != Some("release-assets.githubusercontent.com") {
            return Err(HostError::Identity);
        }
        Ok(())
    }

    #[test]
    fn rejects_untrusted_or_credentialed_redirects() {
        for location in [
            "http://release-assets.githubusercontent.com/x",
            "https://github.com/x",
            "https://user@release-assets.githubusercontent.com/x",
            "https://release-assets.githubusercontent.com:444/x",
            "https://release-assets.githubusercontent.com/x#fragment",
        ] {
            assert_eq!(validate_asset_redirect(location), Err(HostError::Identity));
        }
    }

    #[test]
    fn asset_redirect_request_never_carries_the_api_credential() -> Result<(), HostError> {
        let client = client()?;
        let url = validate_asset_redirect(
            "https://release-assets.githubusercontent.com/releases/download/x?token=ephemeral",
        )?;
        let request = asset_redirect_request(&client, url)
            .build()
            .map_err(|_| HostError::Identity)?;
        if request.headers().contains_key(AUTHORIZATION) {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}
