//! Typed workflow step identity and semantic role.

use super::step::{Step, StepKind};
use crate::errors::ContractError;
use std::collections::BTreeSet;
mod payload;
pub(super) use payload::{valid_cache_tar_launcher, valid_tools_cache_identity};
use payload::{valid_configured_checkout_payload, valid_tool_seed_payload};

/// Fixed local action path used for exact provider-cache admission.
pub const TOFU_PROVIDER_ADMISSION_USES: &str = "./.github/actions/tofu-provider-admission";
/// Fixed local action path for the exact-key host tool seed.
pub const TOOL_SEED_USES: &str = "./.github/actions/velnor-tool-seed";
/// Fixed local action path for the exact V2 tools-cache restore wrapper.
pub const TOOLS_CACHE_RESTORE_USES: &str = "./.github/actions/velnor-tools-cache-restore";
/// Fixed outer step ID exposing the restore wrapper's exact action outputs.
pub const TOOLS_CACHE_RESTORE_ID: &str = "tools-cache-restore";
/// Operation that computes the normal tools-cache payload staging path.
pub const NORMAL_TOOLS_CACHE_STAGE_OPERATION: &str = "normal-tools-cache-stage-path-v1";
/// Operation that checks and imports the normal tools-cache payload.
pub const NORMAL_TOOLS_CACHE_IMPORT_OPERATION: &str = "import-normal-tools-cache-payload-v1";
/// Operation that checks and stages the normal tools-cache payload for saving.
pub const NORMAL_TOOLS_CACHE_EXPORT_OPERATION: &str = "export-normal-tools-cache-payload-v1";
/// Operation that installs verified `tar` aliases for one cache action.
pub const CACHE_TAR_LAUNCHER_INSTALL_OPERATION: &str = "install-cache-tar-aliases-v1";
/// Fixed local composite that installs the verified cache-action aliases.
pub const CACHE_TAR_LAUNCHER_USES: &str = "./.github/actions/velnor-cache-tar-launchers";
/// Composite input carrying the helper's expected SHA-256.
pub const CACHE_TAR_LAUNCHER_SHA256_INPUT: &str = "helper-sha256";
/// Output expression for the verified aliases plus the inherited PATH.
pub const CACHE_TAR_LAUNCHER_PATH_OUTPUT_EXPR: &str =
    "${{ steps.cache-tar-launchers.outputs.path }}";
/// Output expression for the verified pre-seed helper digest.
pub const PRESEED_HELPER_SHA256_OUTPUT_EXPR: &str =
    "${{ steps.preseed-stage.outputs.helper-sha256 }}";
/// Fixed generated V2 identity-and-seed composites for the supported runner lanes.
pub const TOOLS_CACHE_PRELUDE_USES: [&str; 4] = [
    "./.github/actions/velnor-tools-prelude-u22",
    "./.github/actions/velnor-tools-prelude-u24",
    "./.github/actions/velnor-tools-prelude-u26",
    "./.github/actions/velnor-tools-prelude-scale-set",
];
/// Static tool digest input for the runtime-qualified tools identity action.
pub const TOOLS_CACHE_IDENTITY_DIGEST_INPUT: &str = "d";
/// Expression path for the job-private `OpenTofu` plugin cache.
pub const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = super::cache_family::TOFU_PROVIDER_CACHE_BASE_EXPR;
/// Exact-key layer identity shared by the restore and save protocol.
pub const TOFU_PROVIDERS_KEY_PREFIX: &str = "velnor-v1-tofu-providers";
/// Composite output expression for the exact configured provider key.
pub const TOFU_PROVIDERS_KEY_OUTPUT_EXPR: &str = "${{ steps.tofu-providers.outputs.cache-key }}";
/// Composite output expression for the owned provider-cache path.
pub const TOFU_PROVIDERS_PATH_OUTPUT_EXPR: &str = "${{ steps.tofu-providers.outputs.cache-path }}";
/// Run MBX workspace cleanup only after the exact-version/store guard passed.
pub const MBX_WORKSPACE_CLEAN_CONDITION: &str = "always() && steps.mbx-ready.outcome == 'success'";

/// Expression of the App installation token minted for private sources (HD-7).
pub const PRIVATE_TOKEN_OUTPUT_EXPR: &str = "${{ steps.deps-token.outputs.token }}";
/// Expression of the job token bound to the npm registry phase (HD-7).
pub const PRIVATE_NPM_READ_EXPR: &str = "${{ github.token }}";
/// Step env key carrying the App installation token (HD-7).
pub const PRIVATE_FETCH_TOKEN_ENV: &str = "VELNOR_PRIVATE_FETCH_TOKEN";
/// Step env key carrying the npm registry read token (HD-7).
pub const PRIVATE_NPM_READ_TOKEN_ENV: &str = "VELNOR_NPM_READ_TOKEN";
/// Internal operation of the Velnor-owned private fetch step (HD-7).
pub const PRIVATE_FETCH_OPERATION: &str = "private-fetch-v1";
/// Internal operation of the token revocation step (HD-7).
pub const PRIVATE_REVOKE_OPERATION: &str = "private-revoke-v1";
/// Internal operation of the Bun registry phase (HD-6, HD-7).
pub const BUN_REGISTRY_INSTALL_OPERATION: &str = "bun-registry-install-v1";
/// Pinned-action prefix of the private token mint step (HD-7).
pub const PRIVATE_TOKEN_MINT_ACTION_PREFIX: &str = "actions/create-github-app-token@";

pub use super::step_id::{QualificationCacheStepId, QualificationCacheStepStage, StepId};

mod role;
pub use role::StepRole;

/// Check that a step is the configured checkout owner in this workflow scope.
///
/// Checkout is a semantic role, not a display-name convention. The caller
/// supplies the exact pinned action ref from its validated render context.
#[must_use]
pub fn is_configured_checkout(step: &Step, expected_uses: &str) -> bool {
    step.role == Some(StepRole::Checkout)
        && step.condition.is_none()
        && matches!(&step.kind, StepKind::Action { uses, .. } if uses == expected_uses)
        && valid_configured_checkout_payload(&step.kind)
}

/// Check that a step is the unconditional local tool-seed consumer.
#[must_use]
pub fn is_tool_seed_step(step: &Step) -> bool {
    step.role == Some(StepRole::ToolSeed)
        && step.condition.is_none()
        && valid_tool_seed_payload(&step.kind)
}

/// Validate one serialized step scope, including role payloads and unique output IDs.
///
/// Workflow jobs and composite actions use separate output-ID scopes, so renderers
/// call this after internal expansion for each final serialized sequence.
/// # Errors
///
/// Returns a contract error for an invalid role/payload or duplicate step ID.
pub fn validate_step_sequence(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    validate_step_identities(steps, scope)?;
    super::step_mbx_lifecycle::validate_mbx_cleanup_sequence(steps, scope)?;
    super::step_protocol::validate_tofu_provider_sequence(steps, scope)
}

/// Validate IDs and role payloads in a partial serialized scope.
///
/// Use this for a composite body or an expanded workflow scope assembled
/// from only the steps serialized at that level. Call
/// [`validate_step_sequence`] on the full source sequence to enforce any
/// cross-step provider-cache protocol before factoring.
/// # Errors
///
/// Returns a contract error for an invalid role/payload or duplicate output ID.
pub fn validate_step_identity_scope(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    validate_step_identities(steps, scope)
}

/// Validate each step and require unique IDs within one serialized scope.
fn validate_step_identities(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    let mut ids = BTreeSet::new();
    for step in steps {
        step.validate(scope)?;
        if let Some(step_id) = step.id
            && !ids.insert(step_id)
        {
            return Err(ContractError::identity(
                "job.steps.id",
                format!("duplicate_step_id:{scope}:{}", step_id.as_string()),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "step_identity_tests.rs"]
mod tests;
