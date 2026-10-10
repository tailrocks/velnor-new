//! Official Rust channel manifest pins used by hosted qualification.

use velnor_actions_contract::{ReleaseTarget, VelnorConfig};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::{
    MiseSetup, RUST_TOOLCHAIN_QUALIFICATION_MANIFEST_SHA256, RustToolchainQualificationPins,
};

use crate::OrchestratorError;

const QUALIFIED_RUST_VERSION: &str = "1.99.0";

/// Resolve Rust qualification inputs from the active catalog and reviewed manifest digest.
pub(crate) fn resolve(
    config: &VelnorConfig,
) -> Result<RustToolchainQualificationPins, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let rust_version = catalog.version(PinnedTool::Rust).to_owned();
    if rust_version != QUALIFIED_RUST_VERSION {
        return Err(OrchestratorError::Contract {
            problem: format!(
                "rust_toolchain_qualification_requires_manifest_review:{rust_version}"
            ),
        });
    }
    Ok(RustToolchainQualificationPins {
        mise_setup: crate::pins::resolve_mise_setup_for_release_target(
            config,
            ReleaseTarget::LinuxX86_64,
        )?,
        mbx_version: catalog.version(PinnedTool::MrBoxington).to_owned(),
        manifest_url: format!("https://static.rust-lang.org/dist/channel-rust-{rust_version}.toml"),
        rust_version,
        manifest_sha256: RUST_TOOLCHAIN_QUALIFICATION_MANIFEST_SHA256.to_owned(),
    })
}
