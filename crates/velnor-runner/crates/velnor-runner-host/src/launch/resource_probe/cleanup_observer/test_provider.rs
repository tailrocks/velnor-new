use bollard::Docker;

use crate::error::HostError;
use crate::journal::Journal;
use crate::launch::SelectedEngineContext;

/// This provider is for tests. Production has the compiled release provider.
pub(in crate::launch) struct CleanupObserverTestProvider {
    candidate: super::super::provider::VerifiedCandidate,
}

impl CleanupObserverTestProvider {
    pub(in crate::launch) fn new(selected: &SelectedEngineContext) -> Self {
        let revision = "0123456789abcdef0123456789abcdef01234567";
        Self {
            candidate: super::super::provider::VerifiedCandidate {
                runtime_id: format!("sha256:{}", "a".repeat(64)),
                platform: "linux/amd64".to_owned(),
                daemon_architecture: selected.daemon_architecture().to_owned(),
                source_revision: revision.to_owned(),
                binding_fingerprint: "b".repeat(64),
                config: super::super::projection::expected_image_config(revision),
                engine_id: selected.engine_id().to_owned(),
                docker_root_digest: selected.docker_root_digest().to_owned(),
                workspace_cleanup_digest: "c".repeat(64),
            },
        }
    }

    pub(in crate::launch) fn config(&self) -> &bollard::models::ContainerConfig {
        &self.candidate.config
    }
}

impl super::super::provider::ImageProvider for CleanupObserverTestProvider {
    fn verified_candidate(
        &self,
        _docker: &Docker,
        _journal: &Journal,
        _engine_id: &str,
        _info: &bollard::models::SystemInfo,
    ) -> impl std::future::Future<
        Output = Result<Option<super::super::provider::VerifiedCandidate>, HostError>,
    > {
        std::future::ready(Ok(Some(super::super::provider::VerifiedCandidate {
            runtime_id: self.candidate.runtime_id.clone(),
            platform: self.candidate.platform.clone(),
            daemon_architecture: self.candidate.daemon_architecture.clone(),
            source_revision: self.candidate.source_revision.clone(),
            binding_fingerprint: self.candidate.binding_fingerprint.clone(),
            config: self.candidate.config.clone(),
            engine_id: self.candidate.engine_id.clone(),
            docker_root_digest: self.candidate.docker_root_digest.clone(),
            workspace_cleanup_digest: self.candidate.workspace_cleanup_digest.clone(),
        })))
    }
}
