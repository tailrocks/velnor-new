use bollard::{Docker, models::SystemInfo};

use crate::{
    journal::Journal,
    launch::{SelectedEngineContext, resource_probe::provider},
};

use super::{ImageProvider, projection};

pub(in crate::launch) fn intake_composition_image_inspect(
    source_revision: &str,
) -> Result<serde_json::Value, crate::error::HostError> {
    let config = projection::expected_image_config(source_revision);
    let config = serde_json::to_value(config).map_err(|_| crate::error::HostError::Frame)?;
    Ok(serde_json::json!({
        "Id": format!("sha256:{}", "a".repeat(64)),
        "Architecture": "amd64",
        "Os": "linux",
        "Config": config
    }))
}

pub(crate) struct IntakeCompositionProvider {
    candidate: provider::VerifiedCandidate,
}

impl IntakeCompositionProvider {
    pub(crate) fn new(engine: &SelectedEngineContext, source_revision: &str) -> Self {
        Self {
            candidate: provider::VerifiedCandidate {
                runtime_id: format!("sha256:{}", "a".repeat(64)),
                platform: "linux/amd64".to_owned(),
                daemon_architecture: engine.daemon_architecture().to_owned(),
                source_revision: source_revision.to_owned(),
                binding_fingerprint: "b".repeat(64),
                config: projection::expected_image_config(source_revision),
                engine_id: engine.engine_id().to_owned(),
                docker_root_digest: engine.docker_root_digest().to_owned(),
                workspace_cleanup_digest: "c".repeat(64),
            },
        }
    }
}

impl ImageProvider for IntakeCompositionProvider {
    async fn verified_candidate(
        &self,
        _docker: &Docker,
        _journal: &Journal,
        _engine_id: &str,
        _info: &SystemInfo,
    ) -> Result<Option<provider::VerifiedCandidate>, crate::error::HostError> {
        Ok(Some(provider::VerifiedCandidate {
            runtime_id: self.candidate.runtime_id.clone(),
            platform: self.candidate.platform.clone(),
            daemon_architecture: self.candidate.daemon_architecture.clone(),
            source_revision: self.candidate.source_revision.clone(),
            binding_fingerprint: self.candidate.binding_fingerprint.clone(),
            config: self.candidate.config.clone(),
            engine_id: self.candidate.engine_id.clone(),
            docker_root_digest: self.candidate.docker_root_digest.clone(),
            workspace_cleanup_digest: self.candidate.workspace_cleanup_digest.clone(),
        }))
    }
}
