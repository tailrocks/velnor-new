// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use reqwest::{Client, Response, StatusCode, Url};
use serde_json::Value;
use velnor_runner_image_manifest::{ImageEntry, ImageKind, parse_oci_manifest, strict_json};

use super::hash::{is_lower_hex, lowercase_hex, sha256};
use crate::error::HostError;

const MANIFEST_LIMIT: usize = 4 * 1024 * 1024;
const TOKEN_LIMIT: usize = 64 * 1024;
const CONFIG_LIMIT: usize = 64 * 1024 * 1024;
const ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json";

use super::projection_cache::RegistryProjection;

pub(super) async fn verify(
    client: &Client,
    image: &ImageEntry,
    role: ImageKind,
    source: &str,
) -> Result<RegistryProjection, HostError> {
    if image.repository != role.repository()
        || image.registry_digest != image.platform_manifest_digest
        || !digest(&image.registry_digest)
        || !digest(&image.config_digest)
        || !is_lower_hex(source, 40)
    {
        return Err(HostError::Identity);
    }
    let repository = image
        .repository
        .strip_prefix("ghcr.io/")
        .ok_or(HostError::Identity)?;
    let manifest_url = format!(
        "https://ghcr.io/v2/{repository}/manifests/{}",
        image.registry_digest
    );
    let (response, bearer) = manifest_response(client, &manifest_url, repository).await?;
    if response.status() != StatusCode::OK {
        return Err(HostError::Identity);
    }
    let bytes = super::github::read_limited(response, MANIFEST_LIMIT).await?;
    if format!("sha256:{}", lowercase_hex(&sha256(&bytes))) != image.registry_digest {
        return Err(HostError::Identity);
    }
    let value = strict_json::parse_strict_json_bytes(&bytes, MANIFEST_LIMIT)
        .map_err(|_| HostError::Identity)?;
    let (config, _layers) = parse_oci_manifest(&value).map_err(|_| HostError::Identity)?;
    if format!("sha256:{}", config.digest) != image.config_digest
        || config.size > CONFIG_LIMIT as u64
    {
        return Err(HostError::Identity);
    }
    let manifest_bytes = bytes;
    let config_url = format!(
        "https://ghcr.io/v2/{repository}/blobs/{}",
        image.config_digest
    );
    let mut request = client.get(config_url);
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|_| HostError::Identity)?;
    let response = follow_blob_redirect(client, response).await?;
    if response.status() != StatusCode::OK {
        return Err(HostError::Identity);
    }
    let bytes = super::github::read_limited(response, CONFIG_LIMIT).await?;
    RegistryProjection::admit(image, role, source, &manifest_bytes, &bytes)
}

async fn manifest_response(
    client: &Client,
    url: &str,
    repository: &str,
) -> Result<(Response, Option<String>), HostError> {
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, ACCEPT)
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    if response.status() != StatusCode::UNAUTHORIZED {
        return Ok((response, None));
    }
    let challenge = response
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .ok_or(HostError::Identity)?;
    validate_challenge(challenge, repository)?;
    let mut url_token = Url::parse("https://ghcr.io/token").map_err(|_| HostError::Identity)?;
    url_token
        .query_pairs_mut()
        .append_pair("service", "ghcr.io")
        .append_pair("scope", &format!("repository:{repository}:pull"));
    let response = client
        .get(url_token)
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    if response.status() != StatusCode::OK {
        return Err(HostError::Identity);
    }
    let bytes = super::github::read_limited(response, TOKEN_LIMIT).await?;
    let token = strict_json::parse_strict_json_bytes(&bytes, TOKEN_LIMIT)
        .map_err(|_| HostError::Identity)?;
    let token = token
        .get("token")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= 16 * 1024 && !value.chars().any(char::is_control)
        })
        .ok_or(HostError::Identity)?
        .to_owned();
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, ACCEPT)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    Ok((response, Some(token)))
}

async fn follow_blob_redirect(client: &Client, response: Response) -> Result<Response, HostError> {
    if response.status() == StatusCode::OK {
        return Ok(response);
    }
    if !matches!(
        response.status(),
        StatusCode::TEMPORARY_REDIRECT | StatusCode::FOUND
    ) {
        return Err(HostError::Identity);
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(HostError::Identity)?;
    let url = Url::parse(location).map_err(|_| HostError::Identity)?;
    if url.scheme() != "https"
        || url.host_str() != Some("pkg-containers.githubusercontent.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || url.fragment().is_some()
    {
        return Err(HostError::Identity);
    }
    client
        .get(url)
        .send()
        .await
        .map_err(|_| HostError::Identity)
}

fn validate_challenge(challenge: &str, repository: &str) -> Result<(), HostError> {
    let params = challenge
        .strip_prefix("Bearer ")
        .ok_or(HostError::Identity)?;
    let mut values = BTreeMap::new();
    for field in params.split(',') {
        let (key, value) = field.trim().split_once('=').ok_or(HostError::Identity)?;
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or(HostError::Identity)?;
        if values.insert(key, value).is_some() {
            return Err(HostError::Identity);
        }
    }
    if values.len() != 3
        || values.get("realm") != Some(&"https://ghcr.io/token")
        || values.get("service") != Some(&"ghcr.io")
        || values.get("scope") != Some(&format!("repository:{repository}:pull").as_str())
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|value| is_lower_hex(value, 64))
}
