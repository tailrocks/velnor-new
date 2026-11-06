// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;
use sha2::{Digest, Sha256};
use tokio::time::{Instant, timeout_at};

use crate::error::HostError;

use super::{
    ArtifactCandidate, NativeArtifactProvider, hash, registry, runner_docker, runner_release,
};

#[derive(Debug)]
pub struct AdmittedImages {
    pub(crate) runner: String,
    pub(crate) dind: String,
    pub(crate) probe: ArtifactCandidate,
    pub(crate) engine_id: String,
    pub(crate) docker_root: String,
}

impl AdmittedImages {
    pub(crate) async fn verify_engine(&self, docker: &Docker) -> Result<(), HostError> {
        let info = tokio::time::timeout(Duration::from_secs(5), docker.info())
            .await
            .map_err(|_| HostError::DockerTimeout)?
            .map_err(|_| HostError::Docker)?;
        if info.id.as_deref() != Some(self.engine_id.as_str())
            || info.docker_root_dir.as_deref() != Some(self.docker_root.as_str())
        {
            return Err(HostError::Identity);
        }
        Ok(())
    }
}

impl NativeArtifactProvider<'_> {
    pub(crate) async fn bootstrap(&self, docker: &Docker) -> Result<AdmittedImages, HostError> {
        let deadline = Instant::now() + Duration::from_mins(20);
        let release = timeout_at(
            deadline,
            runner_release::fetch_verified(
                &self.client,
                self.pat,
                &self.identity,
                &self.state_directory,
            ),
        )
        .await
        .map_err(|_| HostError::DockerTimeout)??;
        let info = timeout_at(deadline, docker.info())
            .await
            .map_err(|_| HostError::DockerTimeout)?
            .map_err(|_| HostError::Docker)?;
        let engine_id = info
            .id
            .filter(|value| !value.is_empty())
            .ok_or(HostError::Identity)?;
        let docker_root = info
            .docker_root_dir
            .filter(|value| crate::docker_client::canonical_docker_root(value))
            .ok_or(HostError::Path)?;
        let images = &release.manifest.images;
        let mut runtimes = Vec::new();
        for (image, role) in [
            (&images.runner, "runner"),
            (&images.dind, "dind"),
            (&images.resource_probe, "resource-probe"),
        ] {
            timeout_at(deadline, registry::verify_manifest(&self.client, image))
                .await
                .map_err(|_| HostError::DockerTimeout)??;
            let runtime = runner_docker::pull_and_inspect(
                docker,
                image,
                &release.source_sha,
                role,
                &engine_id,
                &docker_root,
                deadline,
            )
            .await?;
            runtimes.push(runtime);
        }
        let mut values = runtimes.into_iter();
        let runner = values.next().ok_or(HostError::Identity)?;
        let dind = values.next().ok_or(HostError::Identity)?;
        let probe = values.next().ok_or(HostError::Identity)?;
        let binding_fingerprint = fingerprint(
            &release.manifest_sha,
            &release.source_sha,
            &engine_id,
            &docker_root,
            &probe.runtime_id,
        );
        Ok(AdmittedImages {
            runner: runner.runtime_id,
            dind: dind.runtime_id,
            probe: ArtifactCandidate {
                runtime_id: probe.runtime_id,
                platform: "linux/amd64".to_owned(),
                source_revision: release.source_sha,
                binding_fingerprint,
                config: probe.config,
            },
            engine_id,
            docker_root,
        })
    }
}

fn fingerprint(manifest: &str, source: &str, engine: &str, root: &str, image: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"velnor-resource-probe-image-binding-v2\0");
    for field in [manifest, source, engine, root, image] {
        let bytes = field.as_bytes();
        digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
        digest.update(bytes);
    }
    hash::lowercase_hex(&digest.finalize())
}

pub async fn admit_runner_images(
    pat: &str,
    state_directory: &std::path::Path,
    docker: &Docker,
) -> Result<AdmittedImages, HostError> {
    NativeArtifactProvider::from_compiled_release_identity(pat, state_directory)?
        .bootstrap(docker)
        .await
}
