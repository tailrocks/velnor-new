// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType};
use bollard::query_parameters::{CreateContainerOptions, UploadToContainerOptions};
use velnor_resource_probe::job_hook::{Challenge, Decision, canonical_json, sha256};
use velnor_runner_host::local::ENVIRONMENT_LABEL;

use super::support::{Fixture, TestResult, suffix};

pub(crate) async fn seed(
    fixture: &Fixture,
    runner: &str,
    challenge: &Challenge,
) -> TestResult<Vec<u8>> {
    let previous = Decision {
        schema_version: 1,
        allow: false,
        challenge: challenge.nonce.clone(),
        request_sha256: sha256(b"previous request"),
        reservation_id: challenge.reservation_id,
        intent_id: challenge.intent_id,
        actual_request_id: 1,
        run_id: 1,
        run_attempt: 1,
        job_key: "ci".to_owned(),
        runner_id: challenge.runner_id,
        policy_generation: challenge.policy_generation,
        policy_digest: challenge.policy_digest.clone(),
        native_job_id: None,
        rest_job_id: None,
        deny_code: Some("previous_decision".to_owned()),
    };
    let bytes = canonical_json(&previous, 8192, "test_decision")?;
    let volume = fixture
        .docker
        .inspect_container(runner, None)
        .await?
        .mounts
        .unwrap_or_default()
        .into_iter()
        .find(|mount| mount.destination.as_deref() == Some("/opt/velnor/source-proof/control"))
        .and_then(|mount| mount.name)
        .ok_or("runner has no metadata volume")?;
    let created = fixture
        .docker
        .create_container(
            Some(CreateContainerOptions {
                name: Some(format!("{}-previous-{}", fixture.environment, suffix()?)),
                platform: super::support::platform().to_owned(),
            }),
            ContainerCreateBody {
                image: Some(fixture.image.clone()),
                entrypoint: Some(vec!["/never-started".to_owned()]),
                labels: Some(HashMap::from([(
                    ENVIRONMENT_LABEL.to_owned(),
                    fixture.environment.clone(),
                )])),
                network_disabled: Some(true),
                host_config: Some(HostConfig {
                    network_mode: Some("none".to_owned()),
                    mounts: Some(vec![Mount {
                        target: Some("/seed".to_owned()),
                        source: Some(volume),
                        typ: Some(MountType::VOLUME),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await?;
    let uploaded = upload(fixture, &created.id, &bytes).await;
    let removed = fixture.docker.remove_container(&created.id, None).await;
    uploaded?;
    removed?;
    Ok(bytes)
}

async fn upload(fixture: &Fixture, container: &str, bytes: &[u8]) -> TestResult<()> {
    let mut header = tar::Header::new_ustar();
    header.set_path("source-proof/control/decision.json")?;
    header.set_uid(0);
    header.set_gid(0);
    header.set_mode(0o444);
    header.set_size(u64::try_from(bytes.len())?);
    header.set_cksum();
    let mut archive = tar::Builder::new(Vec::new());
    archive.append(&header, bytes)?;
    fixture
        .docker
        .upload_to_container(
            container,
            Some(UploadToContainerOptions {
                path: "/seed".to_owned(),
                ..Default::default()
            }),
            bollard::body_full(archive.into_inner()?.into()),
        )
        .await?;
    Ok(())
}
