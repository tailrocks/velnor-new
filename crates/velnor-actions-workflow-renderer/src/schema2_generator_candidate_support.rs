//! Shared inputs and candidate-only renderer for the read-only PR workflow.

use super::Schema2WorkflowRequest;
use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;

#[path = "schema2_generator_release_candidate.rs"]
mod candidate;
#[path = "schema2_generator_candidate_qualification.rs"]
mod qualification;
#[path = "schema2_generator_candidate_steps.rs"]
mod release_steps;
#[path = "schema2_generator_candidate_scripts.rs"]
mod scripts;

const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
const MISE_VERSION: &str = "2026.9.18";
const CHECKOUT_USES: &str = super::features::CHECKOUT_USES;
const ASSET_DIR: &str = "assets";
const LINUX_TARGET: &str = "x86_64-unknown-linux-gnu";
const MACOS_ARM_TARGET: &str = "aarch64-apple-darwin";
const MACOS_X64_TARGET: &str = "x86_64-apple-darwin";
const MACOS_RUNS_ON: &str = "macos-15";
const MACOS_X64_RUNS_ON: &str = "macos-15-intel";

struct AssetNames {
    linux_bin: String,
    linux_sum: String,
    linux_provenance: String,
    macos_arm_bin: String,
    macos_arm_sum: String,
    macos_arm_provenance: String,
    macos_x64_bin: String,
    macos_x64_sum: String,
    macos_x64_provenance: String,
}

impl AssetNames {
    fn for_version(version: &str) -> Self {
        let prefix = format!("velnor-actions-{version}");
        Self {
            linux_bin: format!("{prefix}-{LINUX_TARGET}"),
            linux_sum: format!("{prefix}-{LINUX_TARGET}.sha256"),
            linux_provenance: format!("{prefix}-{LINUX_TARGET}.provenance.json"),
            macos_arm_bin: format!("{prefix}-{MACOS_ARM_TARGET}"),
            macos_arm_sum: format!("{prefix}-{MACOS_ARM_TARGET}.sha256"),
            macos_arm_provenance: format!("{prefix}-{MACOS_ARM_TARGET}.provenance.json"),
            macos_x64_bin: format!("{prefix}-{MACOS_X64_TARGET}"),
            macos_x64_sum: format!("{prefix}-{MACOS_X64_TARGET}.sha256"),
            macos_x64_provenance: format!("{prefix}-{MACOS_X64_TARGET}.provenance.json"),
        }
    }
}

/// Render the isolated pull-request candidate qualification workflow.
pub(super) fn candidate_qualification(
    request: &Schema2WorkflowRequest,
) -> Result<Yaml, RenderError> {
    candidate::workflow(request)
}
