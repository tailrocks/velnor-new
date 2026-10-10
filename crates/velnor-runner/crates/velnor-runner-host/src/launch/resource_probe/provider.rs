//! Narrow image-provider boundary. Production stays closed until verification is qualified.

use bollard::{Docker, models::ContainerConfig, models::SystemInfo};

use crate::error::HostError;

/// Verified release evidence plus the selected daemon's inspected image config.
pub(super) struct VerifiedCandidate {
    pub(super) runtime_id: String,
    pub(super) platform: String,
    pub(super) source_revision: String,
    pub(super) binding_fingerprint: String,
    pub(super) config: ContainerConfig,
}

/// Supplies only an inspected daemon-specific immutable image identity.
pub(super) trait ImageProvider {
    /// Authenticate, load offline, and inspect the selected daemon's image.
    async fn verified_candidate(
        &self,
        docker: &Docker,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError>;
}

/// Fail-closed production provider until the host verifier is independently qualified.
pub(super) struct UnavailableImageProvider;

impl ImageProvider for UnavailableImageProvider {
    async fn verified_candidate(
        &self,
        _docker: &Docker,
        _engine_id: &str,
        _info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        std::future::ready(Ok(None)).await
    }
}
