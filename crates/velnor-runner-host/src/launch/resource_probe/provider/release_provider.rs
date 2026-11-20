// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Docker, HostError, ImageProvider, Journal, PREPARE_TIMEOUT, ProviderFailure,
    ReleaseImageProvider, SystemInfo, VerifiedCandidate, daemon_platform_matches, selected_daemon,
};
use sha2::{Digest, Sha256};

impl ImageProvider for ReleaseImageProvider<'_> {
    async fn verified_candidate(
        &self,
        _docker: &Docker,
        journal: &Journal,
        engine_id: &str,
        info: &SystemInfo,
    ) -> Result<Option<VerifiedCandidate>, HostError> {
        let result: Result<Option<VerifiedCandidate>, ProviderFailure> = async {
            if !daemon_platform_matches(info) {
                return Ok(None);
            }
            let selected = selected_daemon(info, engine_id)?;
            let native = self
                .native
                .as_ref()
                .ok_or(ProviderFailure::NativeAdmission)?;
            let release = tokio::time::timeout(PREPARE_TIMEOUT, native.verified_runner_release())
                .await
                .map_err(|_| ProviderFailure::Timeout)?
                .map_err(|_| ProviderFailure::NativeAdmission)?;
            let selection = super::DaemonSelection::new(
                &selected.engine_id,
                &selected.docker_root_digest,
                &selected.architecture,
            )
            .map_err(|_| ProviderFailure::DaemonIdentity)?;
            let spec = super::registry_spec(
                native,
                &release,
                velnor_runner_image_manifest::ImageKind::ResourceProbe,
                &selection,
            )
            .await?;
            let admitted = super::acquire_registry_image(journal, &self.endpoint, spec)
                .await
                .map_err(|_| ProviderFailure::ImageLoad)?;
            let mut hash = Sha256::new();
            hash.update(b"velnor-resource-probe-registry-binding-v1\0");
            for value in [
                selected.engine_id.as_str(),
                selected.docker_root_digest.as_str(),
                selected.architecture.as_str(),
                release.source_sha.as_str(),
                release.manifest_sha.as_str(),
                admitted.identity.registry_digest.as_str(),
                admitted.identity.config_digest.as_str(),
                admitted.observation.runtime_id(),
            ] {
                hash.update(value.as_bytes());
                hash.update([0]);
            }
            Ok(Some(VerifiedCandidate {
                runtime_id: admitted.observation.runtime_id().to_owned(),
                platform: super::PLATFORM.to_owned(),
                daemon_architecture: selected.architecture,
                source_revision: release.source_sha.clone(),
                binding_fingerprint: super::hex(&hash.finalize()),
                config: admitted.configuration,
                engine_id: selected.engine_id,
                docker_root_digest: selected.docker_root_digest,
                origin: super::evidence::ProbeImageOrigin::Registry {
                    manifest_sha256: release.manifest_sha.clone(),
                },
            }))
        }
        .await;
        match result {
            Ok(Some(candidate)) => Ok(Some(candidate)),
            Ok(None) => {
                eprintln!("resource_probe=unavailable reason=selected_daemon_platform_mismatch");
                Ok(None)
            }
            Err(error) => {
                eprintln!("resource_probe=unavailable reason={}", error.code());
                Err(HostError::Docker)
            }
        }
    }
}
