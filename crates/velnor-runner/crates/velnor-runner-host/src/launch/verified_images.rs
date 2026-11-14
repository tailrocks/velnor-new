// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;

use crate::error::HostError;

use super::SelectedEngineContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedWorkerImages {
    engine_id: String,
    docker_root_digest: String,
    daemon_architecture: String,
    source_sha: String,
    platform: String,
    origin: WorkerImagesOrigin,
    runner: VerifiedImageIdentity,
    dind: VerifiedImageIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::launch) enum WorkerImagesOrigin {
    Registry { manifest_sha256: String },
    Developer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryWorkerImage {
    pub(crate) identity: crate::journal::RegistryImageIdentity,
    pub(crate) observation: crate::journal::RegistryImageObservation,
    pub(crate) projection: crate::artifact_admission::RegistryProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkerImageOrigin {
    Registry(Box<RegistryWorkerImage>),
    Developer,
}

pub(in crate::launch) struct DeveloperWorkerImages {
    pub(in crate::launch) source_sha: String,
    pub(in crate::launch) runner: String,
    pub(in crate::launch) dind: String,
}

impl VerifiedWorkerImages {
    pub(in crate::launch) fn from_registry(
        expected: &SelectedEngineContext,
        source_sha: &str,
        manifest_sha256: &str,
        runner: RegistryWorkerImage,
        dind: RegistryWorkerImage,
    ) -> Result<Self, HostError> {
        use velnor_runner_image_manifest::ImageKind;
        if expected.platform() != "linux/amd64"
            || !lower_hex(source_sha, 40)
            || !lower_hex(manifest_sha256, 64)
            || !registry_matches(
                &runner,
                expected,
                source_sha,
                manifest_sha256,
                ImageKind::Runner,
            )
            || !registry_matches(
                &dind,
                expected,
                source_sha,
                manifest_sha256,
                ImageKind::Dind,
            )
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            engine_id: expected.engine_id().to_owned(),
            docker_root_digest: expected.docker_root_digest().to_owned(),
            daemon_architecture: expected.daemon_architecture().to_owned(),
            source_sha: source_sha.to_owned(),
            platform: expected.platform().to_owned(),
            origin: WorkerImagesOrigin::Registry {
                manifest_sha256: manifest_sha256.to_owned(),
            },
            runner: VerifiedImageIdentity::from_registry(runner)?,
            dind: VerifiedImageIdentity::from_registry(dind)?,
        })
    }

    pub(in crate::launch) fn from_developer(
        expected: &SelectedEngineContext,
        images: DeveloperWorkerImages,
    ) -> Result<Self, HostError> {
        if crate::config::RunnerPlatform::parse(expected.platform()).is_none()
            || !lower_hex(&images.source_sha, 40)
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            engine_id: expected.engine_id().to_owned(),
            docker_root_digest: expected.docker_root_digest().to_owned(),
            daemon_architecture: expected.daemon_architecture().to_owned(),
            source_sha: images.source_sha,
            platform: expected.platform().to_owned(),
            origin: WorkerImagesOrigin::Developer,
            runner: VerifiedImageIdentity::from_developer(images.runner)?,
            dind: VerifiedImageIdentity::from_developer(images.dind)?,
        })
    }

    pub(crate) async fn verify_for_engine(
        &self,
        docker: &Docker,
        expected: &SelectedEngineContext,
    ) -> Result<(), HostError> {
        if !expected.matches_images(self) {
            return Err(HostError::Ownership);
        }
        Box::pin(super::resource_probe::verify_worker_images_for_engine(
            docker, expected, self,
        ))
        .await
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
    }

    pub(crate) fn source_sha(&self) -> &str {
        &self.source_sha
    }

    pub(crate) fn platform(&self) -> &str {
        &self.platform
    }

    pub(crate) fn runner(&self) -> &VerifiedImageIdentity {
        &self.runner
    }

    pub(crate) fn dind(&self) -> &VerifiedImageIdentity {
        &self.dind
    }

    pub(in crate::launch) fn origin(&self) -> &WorkerImagesOrigin {
        &self.origin
    }

    pub(crate) fn registry_manifest_sha256(&self) -> Option<&str> {
        match &self.origin {
            WorkerImagesOrigin::Registry { manifest_sha256 } => Some(manifest_sha256),
            WorkerImagesOrigin::Developer => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedImageIdentity {
    runtime_id: String,
    origin: WorkerImageOrigin,
}

impl VerifiedImageIdentity {
    fn from_registry(image: RegistryWorkerImage) -> Result<Self, HostError> {
        image.observation.validate(&image.identity)?;
        Ok(Self {
            runtime_id: image.observation.runtime_id().to_owned(),
            origin: WorkerImageOrigin::Registry(Box::new(image)),
        })
    }

    fn from_developer(runtime_id: String) -> Result<Self, HostError> {
        if !sha256_digest(&runtime_id) {
            return Err(HostError::Identity);
        }
        Ok(Self {
            runtime_id,
            origin: WorkerImageOrigin::Developer,
        })
    }
    pub(crate) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(crate) fn origin(&self) -> &WorkerImageOrigin {
        &self.origin
    }
}

fn sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| lower_hex(digest, 64))
}

fn registry_matches(
    image: &RegistryWorkerImage,
    expected: &SelectedEngineContext,
    source_sha: &str,
    manifest_sha256: &str,
    role: velnor_runner_image_manifest::ImageKind,
) -> bool {
    let identity = &image.identity;
    identity.engine_id == expected.engine_id()
        && identity.docker_root_digest == expected.docker_root_digest()
        && identity.daemon_architecture == expected.daemon_architecture()
        && identity.source_sha == source_sha
        && identity.release_manifest_sha256 == manifest_sha256
        && identity.role == role
        && image.observation.validate(identity).is_ok()
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
