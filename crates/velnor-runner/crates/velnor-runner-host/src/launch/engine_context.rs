//! Engine identity pinned across verified worker image preparation and stage effects.

use crate::error::HostError;
use crate::journal::EngineContext;
use bollard::models::SystemInfo;

use super::VerifiedWorkerImages;

/// Selected daemon identity paired with its durable host context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedEngineContext {
    engine_id: String,
    endpoint: String,
    platform: String,
    docker_root_digest: String,
    daemon_architecture: String,
}

impl SelectedEngineContext {
    /// Read root and architecture from the independently selected Docker info.
    pub(crate) fn capture(selected: &EngineContext, info: &SystemInfo) -> Result<Self, HostError> {
        let engine_id = info.id.as_deref().ok_or(HostError::Ownership)?;
        let docker_root = info
            .docker_root_dir
            .as_deref()
            .ok_or(HostError::Ownership)?;
        let docker_root_digest = super::resource_probe::docker_root_digest(docker_root)?;
        let daemon_architecture =
            super::resource_probe::daemon_architecture(info).ok_or(HostError::Ownership)?;
        if !super::resource_probe::daemon_platform_matches(info)
            || selected.engine_id != engine_id
            || selected.endpoint.is_empty()
        {
            return Err(HostError::Ownership);
        }
        Ok(Self {
            engine_id: selected.engine_id.clone(),
            endpoint: selected.endpoint.clone(),
            platform: selected.platform.clone(),
            docker_root_digest,
            daemon_architecture: daemon_architecture.to_owned(),
        })
    }

    /// Test-only fixture binding. Production capture uses fresh Docker info.
    #[cfg(test)]
    pub(crate) fn bind(
        selected: &EngineContext,
        images: &VerifiedWorkerImages,
    ) -> Result<Self, HostError> {
        if selected.engine_id != images.engine_id()
            || selected.endpoint.is_empty()
            || selected.platform != images.platform()
            || images.docker_root_digest().len() != 64
            || images.daemon_architecture().is_empty()
        {
            return Err(HostError::Ownership);
        }
        Ok(Self {
            engine_id: selected.engine_id.clone(),
            endpoint: selected.endpoint.clone(),
            platform: selected.platform.clone(),
            docker_root_digest: images.docker_root_digest().to_owned(),
            daemon_architecture: images.daemon_architecture().to_owned(),
        })
    }

    /// Whether the prepared image bundle belongs to this selected engine context.
    #[must_use]
    pub(crate) fn matches_images(&self, images: &VerifiedWorkerImages) -> bool {
        self.engine_id == images.engine_id()
            && self.platform == images.platform()
            && self.docker_root_digest == images.docker_root_digest()
            && self.daemon_architecture == images.daemon_architecture()
    }

    /// Whether this live daemon capture agrees with the durable selection.
    #[must_use]
    pub(crate) fn matches_selection(&self, selected: &EngineContext) -> bool {
        self.engine_id == selected.engine_id
            && self.endpoint == selected.endpoint
            && self.platform == selected.platform
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) fn platform(&self) -> &str {
        &self.platform
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn daemon_architecture(&self) -> &str {
        &self.daemon_architecture
    }
}

#[cfg(test)]
mod tests {
    use crate::journal::EngineContext;
    use crate::launch::verified_images::{
        TestWorkerImageMutation, test_worker_images, test_worker_images_for_root,
        test_worker_images_with,
    };
    use bollard::models::SystemInfo;

    use super::SelectedEngineContext;

    fn selected_context() -> EngineContext {
        EngineContext {
            engine_id: "selected-engine".to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
            platform: "linux/amd64".to_owned(),
        }
    }

    fn info(root: &str, architecture: &str) -> SystemInfo {
        SystemInfo {
            id: Some("selected-engine".to_owned()),
            architecture: Some(architecture.to_owned()),
            docker_root_dir: Some(root.to_owned()),
            os_type: Some("linux".to_owned()),
            ..SystemInfo::default()
        }
    }

    #[test]
    fn bundle_must_match_durable_engine_selection_and_image_identity() {
        let images = test_worker_images();
        let context = SelectedEngineContext::bind(&selected_context(), &images)
            .expect("matching selected engine should bind");
        assert!(context.matches_images(&images));
        for mutation in [
            TestWorkerImageMutation::EngineId,
            TestWorkerImageMutation::DockerRootDigest,
            TestWorkerImageMutation::DaemonArchitecture,
            TestWorkerImageMutation::Platform,
        ] {
            assert!(!context.matches_images(&test_worker_images_with(Some(mutation))));
        }
    }

    #[test]
    fn mismatched_durable_engine_selection_is_rejected() {
        let images = test_worker_images();
        let mut selected = selected_context();
        selected.engine_id = "other-engine".to_owned();
        assert!(SelectedEngineContext::bind(&selected, &images).is_err());
        let mut selected = selected_context();
        selected.platform = "linux/arm64".to_owned();
        assert!(SelectedEngineContext::bind(&selected, &images).is_err());
    }

    #[test]
    fn capture_uses_selected_daemon_root_and_architecture_before_bundle_comparison() {
        let selected = selected_context();
        let context =
            SelectedEngineContext::capture(&selected, &info("/var/lib/docker", "aarch64"))
                .expect("valid selected daemon should be captured");
        let other_root =
            SelectedEngineContext::capture(&selected, &info("/tmp/other-docker", "aarch64"))
                .expect("other valid daemon root should be capturable");
        let other_arch =
            SelectedEngineContext::capture(&selected, &info("/var/lib/docker", "x86_64"))
                .expect("other valid daemon architecture should be capturable");
        let images = test_worker_images_for_root("/var/lib/docker");
        assert!(context.matches_images(&images));
        assert_ne!(
            context.docker_root_digest(),
            other_root.docker_root_digest()
        );
        assert_ne!(
            context.daemon_architecture(),
            other_arch.daemon_architecture()
        );
        assert!(!other_root.matches_images(&images));
        assert!(!other_arch.matches_images(&images));
    }
}
