// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    identity::selected_daemon,
    image_acquisition::{DaemonSelection, ImageReference, ImageSession},
    registry_import::verified_inspection,
};
use crate::{
    artifact_admission::verify_candidate_registry_projection,
    error::HostError,
    journal::{RegistryImageIdentity, RegistryImageObservation},
};
use bollard::models::HealthConfig;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use velnor_runner_image_manifest::{ImageKind, RunnerImageManifest};

#[derive(Debug)]
pub(crate) struct MacFamilyObservation {
    pub(crate) qualification: velnor_runner_qualification::Qualification,
    pub(crate) runner_id: String,
    pub(crate) dind_id: String,
    pub(crate) probe_id: String,
    pub(crate) store: String,
}

pub(crate) async fn qualify_mac_family(
    manifest: RunnerImageManifest,
    source: String,
    manifest_sha: String,
    endpoint: String,
    journal_path: PathBuf,
    transport: std::sync::Arc<dyn crate::artifact_admission::RegistryTransport>,
    end: Instant,
) -> Result<MacFamilyObservation, HostError> {
    let docker = crate::docker_client::connect_unix(&endpoint)?;
    let info = tokio::time::timeout(Duration::from_secs(20), docker.info())
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Docker)?;
    let engine = info.id.as_deref().ok_or(HostError::Identity)?;
    let root = info.docker_root_dir.clone().ok_or(HostError::Identity)?;
    let selected = selected_daemon(&info, engine).map_err(|_| HostError::Identity)?;
    let qualification_session =
        velnor_runner_qualification::open_qualification_session(docker, journal_path, root.clone())
            .await?;
    let selection = DaemonSelection::new(
        &selected.engine_id,
        &selected.docker_root_digest,
        &selected.architecture,
    )?;
    let mut session = ImageSession::connect(&endpoint, &selection).await?;
    let (observations, bindings, healthchecks) = acquire_images(
        &mut session,
        &selected,
        &manifest,
        &source,
        &manifest_sha,
        transport.as_ref(),
        end,
    )
    .await?;
    if observations.len() != 3
        || observations
            .iter()
            .any(|value| value.1 != observations[0].1)
    {
        return Err(HostError::Identity);
    }
    let mut healthchecks = healthchecks.into_iter();
    let health = [
        healthchecks.next().ok_or(HostError::Identity)?,
        healthchecks.next().ok_or(HostError::Identity)?,
        healthchecks.next().ok_or(HostError::Identity)?,
    ];
    let inputs = velnor_runner_qualification::SourceIdentity {
        source_sha: source,
        runner_version: manifest.versions.runner,
        docker_version: manifest.versions.docker,
        buildx_version: manifest.versions.buildx,
        compose_version: manifest.versions.compose,
        node_version: manifest.versions.node,
    };
    let qualification = qualification_session
        .qualify(
            inputs,
            observations[0].0.clone(),
            observations[1].0.clone(),
            observations[2].0.clone(),
            health,
            end,
        )
        .await?;
    if qualification.outer.id != selected.engine_id {
        return Err(HostError::Identity);
    }
    verify_final_images(&mut session, &bindings, &observations, end).await?;
    Ok(MacFamilyObservation {
        qualification,
        runner_id: observations[0].0.clone(),
        dind_id: observations[1].0.clone(),
        probe_id: observations[2].0.clone(),
        store: observations[0].1.to_owned(),
    })
}

async fn verify_final_images(
    session: &mut ImageSession,
    bindings: &[(
        ImageReference,
        RegistryImageIdentity,
        crate::artifact_admission::RegistryProjection,
    )],
    observations: &[(String, &str)],
    end: Instant,
) -> Result<(), HostError> {
    for (index, (reference, identity, projection)) in bindings.iter().enumerate() {
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HostError::DockerTimeout);
        }
        let observation = tokio::time::timeout(remaining, session.inspect(reference))
            .await
            .map_err(|_| HostError::DockerTimeout)??
            .ok_or(HostError::Identity)?;
        let (inspection, _) = verified_inspection(identity, projection, observation)
            .map_err(|_| HostError::Identity)?;
        let (runtime_id, store) = match inspection.observation() {
            RegistryImageObservation::Classic { runtime_id, .. } => (runtime_id, "classic"),
            RegistryImageObservation::Containerd { runtime_id, .. } => (runtime_id, "containerd"),
        };
        if runtime_id != &observations[index].0 || store != observations[index].1 {
            return Err(HostError::Identity);
        }
    }
    Ok(())
}

type Bindings = Vec<(
    ImageReference,
    RegistryImageIdentity,
    crate::artifact_admission::RegistryProjection,
)>;

async fn acquire_images(
    session: &mut ImageSession,
    selected: &super::identity::SelectedDaemon,
    manifest: &RunnerImageManifest,
    source: &str,
    manifest_sha: &str,
    transport: &dyn crate::artifact_admission::RegistryTransport,
    end: Instant,
) -> Result<
    (
        Vec<(String, &'static str)>,
        Bindings,
        Vec<Option<HealthConfig>>,
    ),
    HostError,
> {
    let mut observations = Vec::new();
    let mut bindings = Vec::new();
    let mut healthchecks: Vec<Option<HealthConfig>> = Vec::new();
    for (role, image) in [
        (ImageKind::Runner, &manifest.images.runner),
        (ImageKind::Dind, &manifest.images.dind),
        (ImageKind::ResourceProbe, &manifest.images.resource_probe),
    ] {
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HostError::DockerTimeout);
        }
        let projection = tokio::time::timeout(
            remaining,
            verify_candidate_registry_projection(transport, image, role, source),
        )
        .await
        .map_err(|_| HostError::DockerTimeout)??;
        let reference = ImageReference::new(role, &image.registry_digest)?;
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HostError::DockerTimeout);
        }
        let observation = tokio::time::timeout(remaining, session.pull_and_inspect(&reference))
            .await
            .map_err(|_| HostError::DockerTimeout)??;
        let identity = RegistryImageIdentity::new(
            &selected.engine_id,
            &selected.docker_root_digest,
            &selected.architecture,
            source,
            manifest_sha,
            role,
            image,
        )?;
        let health = crate::resource_probe_release::runner_qualification::image_identity::source_healthcheck(projection.configuration())?;
        let (inspection, _) = verified_inspection(&identity, &projection, observation)
            .map_err(|_| HostError::Identity)?;
        let (runtime_id, store) = match inspection.observation() {
            RegistryImageObservation::Classic { runtime_id, .. } => (runtime_id.clone(), "classic"),
            RegistryImageObservation::Containerd { runtime_id, .. } => {
                (runtime_id.clone(), "containerd")
            }
        };
        observations.push((runtime_id, store));
        bindings.push((reference, identity, projection));
        healthchecks.push(health);
    }
    Ok((observations, bindings, healthchecks))
}
