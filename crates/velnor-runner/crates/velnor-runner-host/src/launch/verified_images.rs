//! Immutable identities supplied only after release and selected Docker engine verification.

use bollard::Docker;

use crate::error::HostError;

use super::SelectedEngineContext;
use super::resource_probe::{VerifiedWorkerImageIdentityProof, VerifiedWorkspaceCleanup};

/// Verified runner and `DinD` images prepared for one selected Docker engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedWorkerImages {
    engine_id: String,
    docker_root_digest: String,
    daemon_architecture: String,
    source_sha: String,
    platform: String,
    release_workspace_cleanup: VerifiedWorkspaceCleanup,
    runner: VerifiedImageIdentity,
    dind: VerifiedImageIdentity,
}

impl VerifiedWorkerImages {
    pub(in crate::launch) fn from_verified_proofs(
        expected: &SelectedEngineContext,
        source_sha: &str,
        release_workspace_cleanup: VerifiedWorkspaceCleanup,
        runner: VerifiedWorkerImageIdentityProof,
        dind: VerifiedWorkerImageIdentityProof,
    ) -> Result<Self, HostError> {
        if expected.platform() != "linux/amd64"
            || !lower_hex(source_sha, 40)
            || !lower_hex(expected.docker_root_digest(), 64)
            || !matches!(expected.daemon_architecture(), "aarch64" | "x86_64")
            || !runner.is_runner()
            || !dind.is_dind()
            || !lower_hex(release_workspace_cleanup.workspace_digest(), 64)
            || !proof_matches(&runner, expected, source_sha)
            || !proof_matches(&dind, expected, source_sha)
        {
            return Err(HostError::Identity);
        }

        Ok(Self {
            engine_id: expected.engine_id().to_owned(),
            docker_root_digest: expected.docker_root_digest().to_owned(),
            daemon_architecture: expected.daemon_architecture().to_owned(),
            source_sha: source_sha.to_owned(),
            platform: expected.platform().to_owned(),
            release_workspace_cleanup,
            runner: VerifiedImageIdentity::from_proof(runner)?,
            dind: VerifiedImageIdentity::from_proof(dind)?,
        })
    }

    /// Recheck the selected Docker engine and both immutable worker image identities.
    pub(crate) async fn verify_for_engine(
        &self,
        docker: &Docker,
        expected: &SelectedEngineContext,
    ) -> Result<(), HostError> {
        if !expected.matches_images(self) {
            return Err(HostError::Ownership);
        }
        super::resource_probe::verify_worker_images_for_engine(docker, expected, self).await
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

    pub(crate) fn release_workspace_cleanup_digest(&self) -> &str {
        self.release_workspace_cleanup.workspace_digest()
    }
}

/// Archive and selected-image identities for one verified worker image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedImageIdentity {
    runtime_id: String,
    archive_sha256: String,
    archive_index_digest: String,
    image_index_digest: Option<String>,
    image_index_size: Option<u64>,
    manifest_digest: String,
    manifest_size: u64,
    config_digest: String,
}

impl VerifiedImageIdentity {
    fn from_proof(proof: VerifiedWorkerImageIdentityProof) -> Result<Self, HostError> {
        let image_index_digest = proof.image_index_digest().to_owned();
        let runtime_id = proof.runtime_id().to_owned();
        let config_digest = proof.config_digest().to_owned();
        if !sha256_digest(&runtime_id)
            || !sha256_digest(&image_index_digest)
            || !sha256_digest(proof.manifest_digest())
            || !sha256_digest(&config_digest)
            || (runtime_id != image_index_digest && runtime_id != config_digest)
            || !lower_hex(proof.archive_sha256(), 64)
            || !lower_hex(proof.archive_index_digest(), 64)
            || proof.image_index_size() == 0
            || proof.manifest_size() == 0
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            runtime_id,
            archive_sha256: proof.archive_sha256().to_owned(),
            archive_index_digest: proof.archive_index_digest().to_owned(),
            image_index_digest: Some(image_index_digest),
            image_index_size: Some(proof.image_index_size()),
            manifest_digest: proof.manifest_digest().to_owned(),
            manifest_size: proof.manifest_size(),
            config_digest,
        })
    }

    pub(crate) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(crate) fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    /// Digest of the outer archive's `index.json` bytes.
    pub(crate) fn archive_index_digest(&self) -> &str {
        &self.archive_index_digest
    }

    /// Digest of the selected nested OCI index, when the archive contains one.
    pub(crate) fn image_index_digest(&self) -> Option<&str> {
        self.image_index_digest.as_deref()
    }

    pub(crate) fn image_index_size(&self) -> Option<u64> {
        self.image_index_size
    }

    pub(crate) fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub(crate) fn manifest_size(&self) -> u64 {
        self.manifest_size
    }

    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }
}

fn proof_matches(
    proof: &VerifiedWorkerImageIdentityProof,
    expected: &SelectedEngineContext,
    source_sha: &str,
) -> bool {
    proof.engine_id() == expected.engine_id()
        && proof.docker_root_digest() == expected.docker_root_digest()
        && proof.daemon_architecture() == expected.daemon_architecture()
        && proof.source_sha() == source_sha
}

fn sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| lower_hex(digest, 64))
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestWorkerImageMutation {
    EngineId,
    DockerRootDigest,
    DaemonArchitecture,
    RunnerClassicStore,
    RunnerRuntimeId,
    RunnerIndexDigest,
    RunnerManifestDigest,
    RunnerConfigDigest,
    Platform,
    SourceSha,
    ReleaseWorkspaceCleanup,
}

#[cfg(test)]
pub(crate) fn test_worker_images() -> VerifiedWorkerImages {
    test_worker_images_with(None)
}

#[cfg(test)]
pub(crate) fn test_worker_images_with(
    mutation: Option<TestWorkerImageMutation>,
) -> VerifiedWorkerImages {
    let mut images = VerifiedWorkerImages {
        engine_id: "selected-engine".to_owned(),
        docker_root_digest: "0".repeat(64),
        daemon_architecture: "aarch64".to_owned(),
        source_sha: "a".repeat(40),
        platform: "linux/amd64".to_owned(),
        release_workspace_cleanup: VerifiedWorkspaceCleanup::test_fixture(),
        runner: test_image("d", "e", Some("f"), "a", "b"),
        dind: test_image("8", "7", Some("9"), "6", "5"),
    };
    match mutation {
        Some(TestWorkerImageMutation::EngineId) => {
            images.engine_id = "different-engine".to_owned();
        }
        Some(TestWorkerImageMutation::DockerRootDigest) => {
            images.docker_root_digest = "1".repeat(64);
        }
        Some(TestWorkerImageMutation::DaemonArchitecture) => {
            images.daemon_architecture = "x86_64".to_owned();
        }
        Some(TestWorkerImageMutation::RunnerClassicStore) => {
            images.runner.runtime_id = images.runner.config_digest.clone();
        }
        Some(TestWorkerImageMutation::RunnerRuntimeId) => {
            images.runner.runtime_id = "mutable:tag".to_owned();
        }
        Some(TestWorkerImageMutation::RunnerIndexDigest) => {
            images.runner.image_index_digest = Some("not-a-digest".to_owned());
        }
        Some(TestWorkerImageMutation::RunnerManifestDigest) => {
            images.runner.manifest_digest = "not-a-digest".to_owned();
        }
        Some(TestWorkerImageMutation::RunnerConfigDigest) => {
            images.runner.config_digest = "not-a-digest".to_owned();
        }
        Some(TestWorkerImageMutation::Platform) => {
            images.platform = "linux/arm64".to_owned();
        }
        Some(TestWorkerImageMutation::SourceSha) => {
            images.source_sha = "not-a-sha".to_owned();
        }
        Some(TestWorkerImageMutation::ReleaseWorkspaceCleanup) => {
            images.release_workspace_cleanup =
                VerifiedWorkspaceCleanup::test_fixture_with_digest("not-a-digest");
        }
        None => {}
    }
    images
}

#[cfg(test)]
pub(crate) fn test_worker_images_for_root(root: &str) -> VerifiedWorkerImages {
    test_worker_images_for_root_with_mutation(root, None)
}

#[cfg(test)]
pub(crate) fn test_worker_images_for_root_with_mutation(
    root: &str,
    mutation: Option<TestWorkerImageMutation>,
) -> VerifiedWorkerImages {
    let mut images = test_worker_images_with(mutation);
    images.docker_root_digest =
        super::resource_probe::docker_root_digest(root).expect("test Docker root is valid");
    images
}

#[cfg(test)]
pub(crate) fn test_worker_images_for(
    engine_id: &str,
    docker_root_digest: &str,
    daemon_architecture: &str,
    platform: &str,
) -> VerifiedWorkerImages {
    let mut images = test_worker_images();
    images.engine_id = engine_id.to_owned();
    images.docker_root_digest = docker_root_digest.to_owned();
    images.daemon_architecture = daemon_architecture.to_owned();
    images.platform = platform.to_owned();
    images
}

#[cfg(test)]
fn test_image(
    archive_nibble: &str,
    archive_index_nibble: &str,
    image_index_nibble: Option<&str>,
    manifest_nibble: &str,
    config_nibble: &str,
) -> VerifiedImageIdentity {
    let runtime_nibble = image_index_nibble.unwrap_or(config_nibble);
    let runtime_id = format!("sha256:{}", runtime_nibble.repeat(64));
    VerifiedImageIdentity {
        runtime_id,
        archive_sha256: archive_nibble.repeat(64),
        archive_index_digest: archive_index_nibble.repeat(64),
        image_index_digest: image_index_nibble
            .map(|nibble| format!("sha256:{}", nibble.repeat(64))),
        image_index_size: image_index_nibble.map(|_| 856),
        manifest_digest: format!("sha256:{}", manifest_nibble.repeat(64)),
        manifest_size: 4_472,
        config_digest: format!("sha256:{}", config_nibble.repeat(64)),
    }
}
