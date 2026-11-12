// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use bollard::models::ContainerConfig;

use super::{PLATFORM, valid_sha};

pub(crate) struct VerifiedCandidate {
    pub(in crate::launch::resource_probe) runtime_id: String,

    pub(in crate::launch::resource_probe) platform: String,

    pub(in crate::launch::resource_probe) daemon_architecture: String,
    pub(in crate::launch::resource_probe) source_revision: String,
    pub(in crate::launch::resource_probe) binding_fingerprint: String,
    pub(in crate::launch::resource_probe) config: ContainerConfig,
    pub(in crate::launch::resource_probe) engine_id: String,
    pub(in crate::launch::resource_probe) docker_root_digest: String,
    pub(in crate::launch::resource_probe) origin: ProbeImageOrigin,
}

#[derive(Debug, Clone)]
pub(in crate::launch) enum ProbeImageOrigin {
    Registry { manifest_sha256: String },
}

impl VerifiedCandidate {
    pub(in crate::launch::resource_probe) fn startup_evidence(
        &self,
    ) -> Option<ProbeImagePreparationEvidence> {
        ProbeImagePreparationEvidence::from_candidate(self)
    }
}

#[derive(Debug, Clone)]
pub(in crate::launch) struct ProbeImagePreparationEvidence {
    runtime_id: String,
    platform: String,
    daemon_architecture: String,
    source_revision: String,
    binding_fingerprint: String,
    engine_id: String,
    docker_root_digest: String,
    origin: ProbeImageOrigin,
}

impl ProbeImagePreparationEvidence {
    #[cfg(test)]
    pub(in crate::launch) fn for_test(engine_id: &str, docker_root_digest: &str) -> Self {
        Self {
            runtime_id: format!("sha256:{}", "3".repeat(64)),
            platform: "linux/amd64".to_owned(),
            daemon_architecture: "x86_64".to_owned(),
            source_revision: "2".repeat(40),
            binding_fingerprint: "4".repeat(64),
            engine_id: engine_id.to_owned(),
            docker_root_digest: docker_root_digest.to_owned(),
            origin: ProbeImageOrigin::Registry {
                manifest_sha256: "5".repeat(64),
            },
        }
    }

    fn from_candidate(candidate: &VerifiedCandidate) -> Option<Self> {
        if !digest_reference(&candidate.runtime_id)
            || !digest(&candidate.binding_fingerprint)
            || !candidate.origin.valid()
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
            origin: candidate.origin.clone(),
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
    pub(in crate::launch) fn origin(&self) -> &ProbeImageOrigin {
        &self.origin
    }
}

impl ProbeImageOrigin {
    fn valid(&self) -> bool {
        match self {
            Self::Registry { manifest_sha256 } => digest(manifest_sha256),
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
