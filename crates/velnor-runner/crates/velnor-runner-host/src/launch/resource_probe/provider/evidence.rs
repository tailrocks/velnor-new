use bollard::models::ContainerConfig;

use super::{PLATFORM, valid_sha};

/// A verified release image plus the selected daemon's inspected image config.
pub(in crate::launch) struct VerifiedCandidate {
    pub(in crate::launch::resource_probe) runtime_id: String,
    /// Verified probe image target. This can differ from daemon architecture under emulation.
    pub(in crate::launch::resource_probe) platform: String,
    /// Canonical architecture reported by the selected Docker engine.
    pub(in crate::launch::resource_probe) daemon_architecture: String,
    pub(in crate::launch::resource_probe) source_revision: String,
    pub(in crate::launch::resource_probe) binding_fingerprint: String,
    pub(in crate::launch::resource_probe) config: ContainerConfig,
    pub(in crate::launch::resource_probe) engine_id: String,
    pub(in crate::launch::resource_probe) docker_root_digest: String,
    pub(in crate::launch::resource_probe) workspace_cleanup_digest: String,
}

impl VerifiedCandidate {
    pub(in crate::launch::resource_probe) fn startup_evidence(
        &self,
    ) -> Option<ProbeImagePreparationEvidence> {
        ProbeImagePreparationEvidence::from_candidate(self)
    }
}

/// Verified probe image evidence for one cleaned release workspace.
#[derive(Debug, Clone)]
pub(in crate::launch) struct ProbeImagePreparationEvidence {
    runtime_id: String,
    platform: String,
    daemon_architecture: String,
    source_revision: String,
    binding_fingerprint: String,
    engine_id: String,
    docker_root_digest: String,
    workspace_cleanup_digest: String,
}

impl ProbeImagePreparationEvidence {
    fn from_candidate(candidate: &VerifiedCandidate) -> Option<Self> {
        if !digest_reference(&candidate.runtime_id)
            || !digest(&candidate.binding_fingerprint)
            || !digest(&candidate.workspace_cleanup_digest)
            || !valid_sha(&candidate.source_revision)
            || candidate.platform != PLATFORM
            || !matches!(candidate.daemon_architecture.as_str(), "aarch64" | "x86_64")
            || candidate.engine_id.is_empty()
            || !digest(&candidate.docker_root_digest)
        {
            return None;
        }
        Some(Self {
            runtime_id: candidate.runtime_id.clone(),
            platform: candidate.platform.clone(),
            daemon_architecture: candidate.daemon_architecture.clone(),
            source_revision: candidate.source_revision.clone(),
            binding_fingerprint: candidate.binding_fingerprint.clone(),
            engine_id: candidate.engine_id.clone(),
            docker_root_digest: candidate.docker_root_digest.clone(),
            workspace_cleanup_digest: candidate.workspace_cleanup_digest.clone(),
        })
    }

    pub(in crate::launch) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }
    pub(in crate::launch) fn platform(&self) -> &str {
        &self.platform
    }
    pub(in crate::launch) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
    }
    pub(in crate::launch) fn source_revision(&self) -> &str {
        &self.source_revision
    }
    pub(in crate::launch) fn binding_fingerprint(&self) -> &str {
        &self.binding_fingerprint
    }
    pub(in crate::launch) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(in crate::launch) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(in crate::launch) fn workspace_cleanup_digest(&self) -> &str {
        &self.workspace_cleanup_digest
    }

    #[cfg(test)]
    pub(in crate::launch::resource_probe) fn test_fixture(
        engine_id: &str,
        docker_root_digest: &str,
        daemon_architecture: &str,
        source_revision: &str,
    ) -> Self {
        Self {
            runtime_id: format!("sha256:{}", "a".repeat(64)),
            platform: PLATFORM.to_owned(),
            daemon_architecture: daemon_architecture.to_owned(),
            source_revision: source_revision.to_owned(),
            binding_fingerprint: "b".repeat(64),
            engine_id: engine_id.to_owned(),
            docker_root_digest: docker_root_digest.to_owned(),
            workspace_cleanup_digest: "c".repeat(64),
        }
    }
}

fn digest_reference(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(digest)
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
