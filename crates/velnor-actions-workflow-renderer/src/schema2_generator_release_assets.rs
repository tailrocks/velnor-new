//! Versioned names and paths for the generator release payload.

use velnor_actions_contract::{GeneratorReleasePlan, GeneratorReleaseTarget};

use crate::RenderError;

pub(super) struct ReleaseAssets {
    pub(super) version: String,
    pub(super) tag: String,
    pub(super) linux_binary: String,
    pub(super) linux_sidecar: String,
    pub(super) macos_binary: String,
    pub(super) macos_sidecar: String,
    pub(super) manifest: String,
    pub(super) linux_bundle: String,
    pub(super) macos_bundle: String,
    pub(super) manifest_bundle: String,
}

impl ReleaseAssets {
    pub(super) fn new(version: &str) -> Result<Self, RenderError> {
        let plan = GeneratorReleasePlan::for_version(version)
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
        let [linux, macos] = plan.targets();
        Ok(Self::from_targets(version, plan.tag(), linux, macos))
    }

    fn from_targets(
        version: &str,
        tag: String,
        linux: GeneratorReleaseTarget,
        macos: GeneratorReleaseTarget,
    ) -> Self {
        let linux_binary = linux.binary_filename(version);
        let macos_binary = macos.binary_filename(version);
        Self {
            version: version.to_owned(),
            tag,
            linux_sidecar: linux.sidecar_filename(version),
            macos_sidecar: macos.sidecar_filename(version),
            linux_bundle: format!("velnor-actions-{version}-linux-provenance.sigstore.json"),
            macos_bundle: format!("velnor-actions-{version}-macos-provenance.sigstore.json"),
            manifest: velnor_actions_contract::RELEASE_MANIFEST_FILENAME.to_owned(),
            manifest_bundle: format!("velnor-actions-{version}-manifest-provenance.sigstore.json"),
            linux_binary,
            macos_binary,
        }
    }

    pub(super) fn linux_assets(&self) -> [&str; 2] {
        [&self.linux_binary, &self.linux_sidecar]
    }

    pub(super) fn macos_assets(&self) -> [&str; 2] {
        [&self.macos_binary, &self.macos_sidecar]
    }

}
