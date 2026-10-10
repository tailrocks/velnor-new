//! Official Rust channel manifest pins used by hosted qualification.

use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::{
    RUST_TOOLCHAIN_QUALIFICATION_MANIFEST_SHA256, RustToolchainQualificationPins,
};

use crate::OrchestratorError;

const QUALIFIED_RUST_VERSION: &str = "1.99.0";

/// Resolve Rust qualification inputs from the active catalog and reviewed manifest digest.
pub(crate) fn resolve() -> Result<RustToolchainQualificationPins, OrchestratorError> {
    let rust_version = ToolCatalog::pinned().version(PinnedTool::Rust).to_owned();
    if rust_version != QUALIFIED_RUST_VERSION {
        return Err(OrchestratorError::Contract {
            problem: format!(
                "rust_toolchain_qualification_requires_manifest_review:{rust_version}"
            ),
        });
    }
    Ok(RustToolchainQualificationPins {
        manifest_url: format!("https://static.rust-lang.org/dist/channel-rust-{rust_version}.toml"),
        rust_version,
        manifest_sha256: RUST_TOOLCHAIN_QUALIFICATION_MANIFEST_SHA256.to_owned(),
    })
}
