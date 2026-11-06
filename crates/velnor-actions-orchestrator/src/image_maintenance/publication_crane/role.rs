// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Manifest, OUTPUT_LIMIT, crane_run, private_docker_config};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_mise::CheckDeadline;

pub(in crate::image_maintenance) fn verify_native_role(
    repository: &str,
    digest: &str,
    source: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let role = match repository {
        "ghcr.io/tailrocks/velnor-job" => "job",
        "ghcr.io/tailrocks/velnor-runner" => "runner",
        "ghcr.io/tailrocks/velnor-dind" => "dind",
        "ghcr.io/tailrocks/velnor-resource-probe" => "resource_probe",
        _ => return Ok(()),
    };
    let config = private_docker_config()?;
    let gateway = super::gateway::RegistryGateway::start(&config, repository, deadline)?;
    let docker_config = gateway.config();
    let repository = gateway.repository(repository);
    let manifest_output = crane_run(
        &docker_config,
        super::CraneOperation::Manifest(&format!("{repository}@{digest}")),
        deadline,
    )?;
    if !manifest_output.success
        || manifest_output.code != Some(0)
        || manifest_output.signal.is_some()
        || super::digest(&manifest_output.stdout) != digest
    {
        return Err(internal("image_promotion_manifest"));
    }
    let manifest: Manifest = serde_json::from_slice(&manifest_output.stdout)
        .map_err(|_| internal("image_promotion_manifest"))?;
    if manifest.schema_version != 2 || manifest.config.size > OUTPUT_LIMIT as u64 {
        return Err(internal("image_promotion_manifest"));
    }
    let config_output = crane_run(
        &docker_config,
        super::CraneOperation::Blob(&format!("{repository}@{}", manifest.config.digest)),
        deadline,
    )?;
    if !config_output.success
        || config_output.code != Some(0)
        || config_output.signal.is_some()
        || config_output.stdout.len() as u64 != manifest.config.size
        || super::digest(&config_output.stdout) != manifest.config.digest
    {
        return Err(internal("image_promotion_config_digest"));
    }
    let image: serde_json::Value = serde_json::from_slice(&config_output.stdout)
        .map_err(|_| internal("image_promotion_image_kind"))?;
    let config = image
        .get("config")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| internal("image_promotion_image_kind"))?;
    let labels = config
        .get("Labels")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| internal("image_promotion_image_kind"))?;
    if labels
        .get("org.opencontainers.image.revision")
        .and_then(serde_json::Value::as_str)
        != Some(source)
    {
        return Err(internal("image_promotion_image_kind"));
    }
    if role == "resource_probe" {
        let expected = serde_json::json!({
            "User": "65532:65532", "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
            "Entrypoint": ["/velnor/resource-probe"], "WorkingDir": "/", "Labels": {"org.opencontainers.image.revision": source}
        });
        if image.get("config") != Some(&expected) {
            return Err(internal("image_promotion_image_kind"));
        }
    } else if labels
        .get("io.velnor.image.kind")
        .and_then(serde_json::Value::as_str)
        != Some(role)
    {
        return Err(internal("image_promotion_image_kind"));
    }
    Ok(())
}
