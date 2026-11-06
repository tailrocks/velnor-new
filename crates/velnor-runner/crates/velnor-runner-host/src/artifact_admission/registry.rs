// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use reqwest::{Client, StatusCode};
use serde_json::Value;

use crate::error::HostError;
use crate::resource_probe_release::runner_manifest::model::ImageEntry;

use super::hash::{lowercase_hex, sha256};

const MANIFEST_LIMIT: usize = 4 * 1024 * 1024;
const TOKEN_LIMIT: usize = 64 * 1024;
const ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json";

pub(super) async fn verify_manifest(client: &Client, image: &ImageEntry) -> Result<(), HostError> {
    let repository = image
        .repository
        .strip_prefix("ghcr.io/")
        .ok_or(HostError::Identity)?;
    if !matches!(
        repository,
        "tailrocks/velnor-runner" | "tailrocks/velnor-dind" | "tailrocks/velnor-resource-probe"
    ) {
        return Err(HostError::Identity);
    }
    let url = format!(
        "https://ghcr.io/v2/{repository}/manifests/{}",
        image.registry_digest
    );
    let first = client
        .get(&url)
        .header(reqwest::header::ACCEPT, ACCEPT)
        .send()
        .await
        .map_err(|_| HostError::Identity)?;
    let response = if first.status() == StatusCode::UNAUTHORIZED {
        let challenge = first
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .ok_or(HostError::Identity)?;
        validate_challenge(challenge, repository)?;
        let scope = format!("repository:{repository}:pull");
        let mut token_url =
            reqwest::Url::parse("https://ghcr.io/token").map_err(|_| HostError::Identity)?;
        token_url
            .query_pairs_mut()
            .append_pair("service", "ghcr.io")
            .append_pair("scope", &scope);
        let token_response = client
            .get(token_url)
            .send()
            .await
            .map_err(|_| HostError::Identity)?;
        if token_response.status() != StatusCode::OK {
            return Err(HostError::Identity);
        }
        let token_bytes = super::github::read_limited(token_response, TOKEN_LIMIT).await?;
        let token_json = super::strict_json::parse(&token_bytes)?;
        let token = token_json
            .get("token")
            .and_then(Value::as_str)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 16 * 1024
                    && !value.chars().any(char::is_control)
            })
            .ok_or(HostError::Identity)?;
        client
            .get(&url)
            .header(reqwest::header::ACCEPT, ACCEPT)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| HostError::Identity)?
    } else {
        first
    };
    if response.status() != StatusCode::OK {
        return Err(HostError::Identity);
    }
    let bytes = super::github::read_limited(response, MANIFEST_LIMIT).await?;
    validate_manifest_bytes(&bytes, image)
}

fn validate_manifest_bytes(bytes: &[u8], image: &ImageEntry) -> Result<(), HostError> {
    if format!("sha256:{}", lowercase_hex(&sha256(bytes))) != image.registry_digest {
        return Err(HostError::Identity);
    }
    let manifest = super::strict_json::parse(bytes)?;
    let media_type = manifest.get("mediaType").and_then(Value::as_str);
    if manifest.get("schemaVersion").and_then(Value::as_u64) != Some(2)
        || !matches!(
            media_type,
            Some(
                "application/vnd.oci.image.manifest.v1+json"
                    | "application/vnd.docker.distribution.manifest.v2+json"
            )
        )
        || manifest
            .get("config")
            .and_then(|config| config.get("digest"))
            .and_then(Value::as_str)
            != Some(image.config_digest.as_str())
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_challenge(challenge: &str, repository: &str) -> Result<(), HostError> {
    let params = challenge
        .strip_prefix("Bearer ")
        .ok_or(HostError::Identity)?;
    let mut values = std::collections::BTreeMap::new();
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
    if values.get("realm") != Some(&"https://ghcr.io/token")
        || values.get("service") != Some(&"ghcr.io")
        || values.get("scope") != Some(&format!("repository:{repository}:pull").as_str())
        || values.len() != 3
    {
        return Err(HostError::Identity);
    }
    Ok(())
}
