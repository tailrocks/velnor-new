use super::super::archive::worker::{ParsedWorkerArchive, WorkerImage};
use super::super::identity::SelectedDaemon;

pub(super) fn image_proof(
    selected: &SelectedDaemon,
    source_sha: &str,
    image: WorkerImage,
    archive: &ParsedWorkerArchive,
    runtime_id: String,
) -> VerifiedWorkerImageIdentityProof {
    VerifiedWorkerImageIdentityProof {
        engine_id: selected.engine_id.clone(),
        docker_root_digest: selected.docker_root_digest.clone(),
        daemon_architecture: selected.architecture.clone(),
        source_sha: source_sha.to_owned(),
        image,
        runtime_id,
        archive_sha256: archive.archive_sha256.clone(),
        archive_index_digest: archive.archive_index_digest.clone(),
        image_index_digest: archive.image_index_digest.clone(),
        image_index_size: archive.image_index_size,
        manifest_digest: archive.manifest_digest.clone(),
        manifest_size: archive.manifest_size,
        config_digest: archive.config_digest.clone(),
    }
}

/// One worker image identity after the release archive and selected store pass verification.
pub(in crate::launch) struct VerifiedWorkerImageIdentityProof {
    engine_id: String,
    docker_root_digest: String,
    daemon_architecture: String,
    source_sha: String,
    image: WorkerImage,
    runtime_id: String,
    archive_sha256: String,
    archive_index_digest: String,
    image_index_digest: String,
    image_index_size: u64,
    manifest_digest: String,
    manifest_size: u64,
    config_digest: String,
}

impl VerifiedWorkerImageIdentityProof {
    #[cfg(test)]
    pub(in crate::launch::resource_probe::provider) fn test_fixture(
        selected: &SelectedDaemon,
        source_sha: &str,
        image: WorkerImage,
        nibble: u8,
    ) -> Self {
        let image_index_digest = fixture_digest(nibble);
        Self {
            engine_id: selected.engine_id.clone(),
            docker_root_digest: selected.docker_root_digest.clone(),
            daemon_architecture: selected.architecture.clone(),
            source_sha: source_sha.to_owned(),
            image,
            runtime_id: image_index_digest.clone(),
            archive_sha256: fixture_hex(nibble),
            archive_index_digest: fixture_hex(nibble + 1),
            image_index_digest,
            image_index_size: 856,
            manifest_digest: fixture_digest(nibble + 2),
            manifest_size: 4_472,
            config_digest: fixture_digest(nibble + 3),
        }
    }

    pub(in crate::launch) fn is_runner(&self) -> bool {
        self.image == WorkerImage::Runner
    }

    pub(in crate::launch) fn is_dind(&self) -> bool {
        self.image == WorkerImage::Dind
    }

    pub(in crate::launch) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(in crate::launch) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(in crate::launch) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
    }

    pub(in crate::launch) fn source_sha(&self) -> &str {
        &self.source_sha
    }

    pub(in crate::launch) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(in crate::launch) fn archive_sha256(&self) -> &str {
        &self.archive_sha256
    }

    pub(in crate::launch) fn archive_index_digest(&self) -> &str {
        &self.archive_index_digest
    }

    pub(in crate::launch) fn image_index_digest(&self) -> &str {
        &self.image_index_digest
    }

    pub(in crate::launch) fn image_index_size(&self) -> u64 {
        self.image_index_size
    }

    pub(in crate::launch) fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub(in crate::launch) fn manifest_size(&self) -> u64 {
        self.manifest_size
    }

    pub(in crate::launch) fn config_digest(&self) -> &str {
        &self.config_digest
    }
}

#[cfg(test)]
fn fixture_digest(nibble: u8) -> String {
    format!("sha256:{}", fixture_hex(nibble))
}

#[cfg(test)]
fn fixture_hex(nibble: u8) -> String {
    format!("{nibble:x}").repeat(64)
}
