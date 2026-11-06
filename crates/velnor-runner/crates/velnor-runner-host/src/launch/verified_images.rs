// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::Docker;

use crate::error::HostError;

use super::SelectedEngineContext;
use super::resource_probe::{VerifiedWorkerImageIdentityProof, VerifiedWorkspaceCleanup};

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

    pub(crate) async fn from_journal_for_engine(
        journal: &crate::journal::Journal,
        docker: &Docker,
        expected: &SelectedEngineContext,
    ) -> Result<Self, HostError> {
        let (runner_record, dind_record) = journal
            .latest_completed_worker_image_pair(
                expected.engine_id(),
                expected.docker_root_digest(),
                expected.daemon_architecture(),
                expected.platform(),
            )
            .await?
            .ok_or(HostError::Identity)?;
        let source_sha = runner_record.request.source_sha.clone();
        if expected.platform() != "linux/amd64"
            || !lower_hex(&source_sha, 40)
            || !lower_hex(expected.docker_root_digest(), 64)
            || !matches!(expected.daemon_architecture(), "aarch64" | "x86_64")
            || dind_record.request.source_sha != source_sha
        {
            return Err(HostError::Identity);
        }
        let images = Self {
            engine_id: expected.engine_id().to_owned(),
            docker_root_digest: expected.docker_root_digest().to_owned(),
            daemon_architecture: expected.daemon_architecture().to_owned(),
            source_sha,
            platform: expected.platform().to_owned(),
            release_workspace_cleanup: VerifiedWorkspaceCleanup::for_path(journal.path()),
            runner: VerifiedImageIdentity::from_record(runner_record)?,
            dind: VerifiedImageIdentity::from_record(dind_record)?,
        };
        images.verify_for_engine(docker, expected).await?;
        Ok(images)
    }

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
    #[expect(
        clippy::needless_pass_by_value,
        reason = "transfers ownership of the verified image proof into its checked receipt"
    )]
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

    fn from_record(record: crate::journal::ImageImportRecord) -> Result<Self, HostError> {
        if record.state != crate::journal::ImageImportState::Completed {
            return Err(HostError::Identity);
        }
        let runtime_id = record.image_id.ok_or(HostError::Identity)?;
        let request = record.request;
        let image_index_digest = request.image_index_digest.ok_or(HostError::Identity)?;
        let image_index_size = request.image_index_size.ok_or(HostError::Identity)?;
        if !sha256_digest(&runtime_id)
            || !sha256_digest(&image_index_digest)
            || !sha256_digest(&request.manifest_digest)
            || !sha256_digest(&request.config_digest)
            || (runtime_id != image_index_digest && runtime_id != request.config_digest)
            || !lower_hex(&request.archive_sha256, 64)
            || !lower_hex(&request.archive_index_digest, 64)
            || image_index_size == 0
            || request.manifest_size == 0
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            runtime_id,
            archive_sha256: request.archive_sha256,
            archive_index_digest: request.archive_index_digest,
            image_index_digest: Some(image_index_digest),
            image_index_size: Some(image_index_size),
            manifest_digest: request.manifest_digest,
            manifest_size: request.manifest_size,
            config_digest: request.config_digest,
        })
    }

    pub(crate) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(crate) fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    pub(crate) fn archive_index_digest(&self) -> &str {
        &self.archive_index_digest
    }

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
