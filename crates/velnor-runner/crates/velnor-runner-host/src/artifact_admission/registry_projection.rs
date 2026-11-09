// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use reqwest::{Client, Url};
use serde_json::Value;
use velnor_runner_image_manifest::{ImageEntry, ImageKind, parse_oci_manifest, strict_json};

use super::hash::{is_lower_hex, lowercase_hex, sha256};
use crate::error::HostError;

const MANIFEST_LIMIT: usize = 4 * 1024 * 1024;
const TOKEN_LIMIT: usize = 64 * 1024;
const CONFIG_LIMIT: usize = 64 * 1024 * 1024;

use super::projection_cache::RegistryProjection;
use super::registry_transport::{
    AnonymousRegistryTransport, RegistryRequest, RegistryResponse, RegistryTransport,
};

pub(super) async fn verify(
    client: &Client,
    image: &ImageEntry,
    role: ImageKind,
    source: &str,
) -> Result<RegistryProjection, HostError> {
    RegistryProjection::fetch(
        &AnonymousRegistryTransport(client.clone()),
        image,
        role,
        source,
    )
    .await
}

impl RegistryProjection {
    pub async fn fetch(
        transport: &dyn RegistryTransport,
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
        let (response, bearer) = manifest_response(transport, &manifest_url, repository).await?;
        if response.status != 200 {
            return Err(HostError::Identity);
        }
        let bytes = response.body;
        if format!("sha256:{}", lowercase_hex(&sha256(&bytes))) != image.registry_digest {
            return Err(HostError::Identity);
        }
        let value = strict_json::parse_strict_json_bytes(&bytes, MANIFEST_LIMIT)
            .map_err(|_| HostError::Identity)?;
        let (config, _) = parse_oci_manifest(&value).map_err(|_| HostError::Identity)?;
        if format!("sha256:{}", config.digest) != image.config_digest
            || config.size > CONFIG_LIMIT as u64
        {
            return Err(HostError::Identity);
        }
        let config_url = format!(
            "https://ghcr.io/v2/{repository}/blobs/{}",
            image.config_digest
        );
        let response = get(transport, config_url, false, bearer, CONFIG_LIMIT).await?;
        let response = follow_blob_redirect(transport, response).await?;
        if response.status != 200 {
            return Err(HostError::Identity);
        }
        RegistryProjection::from_oci(image, role, source, &bytes, &response.body)
    }
}

async fn manifest_response(
    transport: &dyn RegistryTransport,
    url: &str,
    repository: &str,
) -> Result<(RegistryResponse, Option<String>), HostError> {
    let response = get(transport, url.to_owned(), true, None, MANIFEST_LIMIT).await?;
    if response.status != 401 {
        return Ok((response, None));
    }
    validate_challenge(
        response.challenge.as_deref().ok_or(HostError::Identity)?,
        repository,
    )?;
    let mut token_url = Url::parse("https://ghcr.io/token").map_err(|_| HostError::Identity)?;
    token_url
        .query_pairs_mut()
        .append_pair("service", "ghcr.io")
        .append_pair("scope", &format!("repository:{repository}:pull"));
    let response = get(transport, token_url.into(), false, None, TOKEN_LIMIT).await?;
    if response.status != 200 {
        return Err(HostError::Identity);
    }
    let value = strict_json::parse_strict_json_bytes(&response.body, TOKEN_LIMIT)
        .map_err(|_| HostError::Identity)?;
    let token = value
        .get("token")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= 16 * 1024 && !value.chars().any(char::is_control)
        })
        .ok_or(HostError::Identity)?
        .to_owned();
    let response = get(
        transport,
        url.to_owned(),
        true,
        Some(token.clone()),
        MANIFEST_LIMIT,
    )
    .await?;
    Ok((response, Some(token)))
}

async fn follow_blob_redirect(
    transport: &dyn RegistryTransport,
    response: RegistryResponse,
) -> Result<RegistryResponse, HostError> {
    if response.status == 200 {
        return Ok(response);
    }
    if !matches!(response.status, 302 | 307) {
        return Err(HostError::Identity);
    }
    let url = Url::parse(response.location.as_deref().ok_or(HostError::Identity)?)
        .map_err(|_| HostError::Identity)?;
    if url.scheme() != "https"
        || url.host_str() != Some("pkg-containers.githubusercontent.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || url.fragment().is_some()
    {
        return Err(HostError::Identity);
    }
    get(transport, url.into(), false, None, CONFIG_LIMIT).await
}

async fn get(
    transport: &dyn RegistryTransport,
    url: String,
    accept_manifest: bool,
    bearer: Option<String>,
    body_limit: usize,
) -> Result<RegistryResponse, HostError> {
    let response = transport
        .get(RegistryRequest {
            url,
            accept_manifest,
            bearer,
            body_limit,
        })
        .await?;
    if response.body.len() > body_limit {
        return Err(HostError::Frame);
    }
    Ok(response)
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
