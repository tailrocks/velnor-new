// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::ProviderFailure;
use super::model::{BASE_LABELS, PATH_ENV, WorkerImage};
use super::valid_sha;

pub fn verify_worker_runtime_config(
    value: &Value,
    image: WorkerImage,
    source_sha: &str,
) -> Result<(), crate::error::HostError> {
    verify_runtime_config(value, image, source_sha).map_err(|_| crate::error::HostError::Docker)
}

fn verify_runtime_config(
    value: &Value,
    image: WorkerImage,
    source_sha: &str,
) -> Result<(), ProviderFailure> {
    if !valid_sha(source_sha) {
        return Err(ProviderFailure::Archive);
    }
    let object = value.as_object().ok_or(ProviderFailure::Archive)?;
    let expected_fields = match image {
        WorkerImage::Runner => ["User", "Env", "Entrypoint", "WorkingDir", "Labels"].as_slice(),
        WorkerImage::Dind => ["Env", "Entrypoint", "Labels"].as_slice(),
    };
    if object.len() != expected_fields.len()
        || object
            .keys()
            .any(|field| !expected_fields.contains(&field.as_str()))
        || value.get("User").and_then(Value::as_str) != image.user()
        || value.get("WorkingDir").and_then(Value::as_str) != image.working_dir()
        || value
            .get("Env")
            .and_then(Value::as_array)
            .is_none_or(|env| {
                env.len() != 1 || env.first().and_then(Value::as_str) != Some(PATH_ENV)
            })
        || value
            .get("Entrypoint")
            .and_then(Value::as_array)
            .is_none_or(|entrypoint| {
                entrypoint.len() != 1
                    || entrypoint.first().and_then(Value::as_str) != Some(image.entrypoint())
            })
    {
        return Err(ProviderFailure::Archive);
    }
    let labels = value
        .get("Labels")
        .and_then(Value::as_object)
        .ok_or(ProviderFailure::Archive)?;
    if labels.len() != BASE_LABELS.len()
        || labels
            .keys()
            .any(|label| !BASE_LABELS.contains(&label.as_str()))
        || labels
            .get("org.opencontainers.image.created")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || labels
            .get("org.opencontainers.image.description")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || labels
            .get("org.opencontainers.image.title")
            .and_then(Value::as_str)
            != Some("ubuntu")
        || labels
            .get("org.opencontainers.image.version")
            .and_then(Value::as_str)
            != Some("26.04")
        || labels.get("io.velnor.image.kind").and_then(Value::as_str) != Some(image.image_kind())
        || labels
            .get("org.opencontainers.image.revision")
            .and_then(Value::as_str)
            != Some(source_sha)
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(())
}
