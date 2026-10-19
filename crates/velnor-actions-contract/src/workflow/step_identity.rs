//! Typed workflow step identity and semantic role.

use super::step::{Step, StepKind};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
mod payload;
use payload::{
    action_has_prefix_for_kind, internal_operation, valid_checkout_payload,
    valid_configured_checkout_payload, valid_mbx_cache, valid_mise_setup, valid_tool_seed_payload,
};
pub(super) use payload::{valid_cache_tar_launcher, valid_tools_cache_identity};

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

pub use super::step_id::StepId;

/// Semantic step role used for workflow composition and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepRole {
    /// Checkout whose credentials are disabled.
    Checkout,
    /// Copy from the trusted, exact-key host tool seed.
    ToolSeed,
    /// Install the exact catalog tools used by later steps.
    PreparePinnedTools,
    /// Install the pinned Rust components used by task work.
    PrepareRustComponents,
    /// Pinned Mise action that owns its built-in tools cache.
    MiseSetup,
    /// Authenticated cargo-deny analyzer bootstrap.
    CargoDeny,
    /// Authenticated cargo-machete analyzer bootstrap.
    CargoMachete,
    /// Authenticated actionlint analyzer bootstrap.
    Actionlint,
    /// Authenticated zizmor analyzer bootstrap.
    Zizmor,
    /// Authenticated markdownlint analyzer bootstrap.
    Markdownlint,
    /// Authenticated strict-JSON check bootstrap.
    StrictJson,
    /// Authenticated frontmatter-ID check bootstrap.
    FrontmatterId,
    /// Authenticated link-check analyzer bootstrap.
    LinkCheck,
    /// Authenticated native-validators probe bootstrap.
    NativeValidators,
    /// Planner producing the plan outputs.
    PlanProducer,
    /// Baseline publisher producing the artifact name output.
    BaselinePublisher,
    /// Digest-verified Velnor executable acquisition.
    AcquireVelnor,
    /// Pre-seed copy of the helper into runner-temp staging.
    PreseedStage,
    /// Download of the plan artifact for final merging.
    DownloadPlan,
    /// Generated-tree freshness check.
    CheckGenerated,
    /// Upload of the planner report artifact.
    PublishPlan,
    /// Upload of one matrix task report.
    MatrixReportUpload,
    /// Download of a candidate attestation artifact.
    AttestationDownload,
    /// Fetch of exact matrix report artifacts.
    FetchReports,
    /// Upload of the final merged report.
    PublishFinal,
    /// `OpenTofu`/Rust plan formatting step.
    PlanFormat,
    /// Per-crate minimum-supported-Rust-version qualification.
    MsrvQualification,
    /// One elected tools-cache writer.
    ToolsCacheSave,
    /// Installer producing PATH output for cache-action steps only.
    CacheTarLaunchers,
    /// Runtime identity action that gates the V2 tools-cache layer.
    ToolsCacheIdentity,
    /// Read-only restore wrapper for the V2 tools-cache payload.
    ToolsCacheRestore,
    /// CLI operation that computes the normal tools-cache staging path.
    NormalToolsCacheStage,
    /// CLI operation that checks and imports the normal tools-cache payload.
    NormalToolsCacheImport,
    /// CLI operation that checks and stages the normal tools-cache payload.
    NormalToolsCacheExport,
    /// Restore of the shared Cargo registry and git sources snapshot.
    CargoSourcesRestore,
    /// Single elected writer for the shared Cargo sources snapshot.
    CargoSourcesSave,
    /// Fetch probe used to populate the Cargo source snapshot on a miss.
    CargoSourcesFetch,
    /// Compute runtime cache trust and keys before cache restores.
    CacheKeys,
    /// One elected `OpenTofu` provider-cache writer.
    TofuProvidersSave,
    /// `OpenTofu` provider-cache restore/admission composite before provider use.
    TofuProvidersRestore,
    /// `OpenTofu` init/validate shell that consumes the admitted plugin cache.
    TofuProviderUse,
    /// Pre-seed build of the candidate helper.
    PreseedBuild,
    /// Pre-seed manifest creation.
    PreseedManifest,
    /// Verification of the freshly built pre-seed candidate.
    PreseedVerifyBuild,
    /// Pre-seed candidate artifact upload.
    PreseedUpload,
    /// Pre-seed candidate artifact download.
    PreseedDownload,
    /// Pre-seed manifest verification.
    PreseedVerifyManifest,
    /// Native MBX action owns hosted and local object-cache lifecycle.
    MbxCache,
    /// Exact MBX binary and object-store verification after the native action.
    MbxVersionCheck,
    /// Exact MBX and Rust toolchain preflight.
    MbxPreflight,
    /// Final workspace-target cleanup before the native MBX action post step.
    MbxWorkspaceCleanup,
}

impl StepRole {
    /// Check role-specific payload kind and identity.
    pub(crate) fn validate(self, kind: &StepKind, job: &str) -> Result<(), ContractError> {
        if self.matches_payload(kind) {
            Ok(())
        } else {
            Err(ContractError::identity(
                "step.role",
                format!("role_kind_mismatch:{job}:{self:?}"),
            ))
        }
    }

    /// Match one role against its fixed kind and payload family.
    fn matches_payload(self, kind: &StepKind) -> bool {
        match self {
            Self::Checkout => valid_checkout_payload(kind),
            Self::ToolSeed => valid_tool_seed_payload(kind),
            Self::MiseSetup => valid_mise_setup(kind),
            Self::TofuProviderUse => super::step_protocol::valid_provider_use(kind),
            Self::PlanProducer => internal_operation(kind, "plan-v1"),
            Self::BaselinePublisher => internal_operation(kind, "publish-baseline-v1"),
            Self::FetchReports => internal_operation(kind, "fetch-reports-v1"),
            Self::DownloadPlan | Self::AttestationDownload | Self::PreseedDownload => {
                action_has_prefix_for_kind(kind, "actions/download-artifact@")
            }
            Self::PublishPlan
            | Self::MatrixReportUpload
            | Self::PublishFinal
            | Self::PreseedUpload => action_has_prefix_for_kind(kind, "actions/upload-artifact@"),
            Self::ToolsCacheSave | Self::CargoSourcesSave => {
                action_has_prefix_for_kind(kind, "actions/cache/save@")
            }
            Self::ToolsCacheIdentity => valid_tools_cache_identity(kind),
            Self::ToolsCacheRestore => matches!(
                kind,
                StepKind::Action { uses, .. } if uses == TOOLS_CACHE_RESTORE_USES
            ),
            Self::NormalToolsCacheStage => {
                internal_operation(kind, NORMAL_TOOLS_CACHE_STAGE_OPERATION)
            }
            Self::NormalToolsCacheImport => {
                internal_operation(kind, NORMAL_TOOLS_CACHE_IMPORT_OPERATION)
            }
            Self::NormalToolsCacheExport => {
                internal_operation(kind, NORMAL_TOOLS_CACHE_EXPORT_OPERATION)
            }
            Self::CacheTarLaunchers => valid_cache_tar_launcher(kind),
            Self::CargoSourcesRestore => action_has_prefix_for_kind(kind, "actions/cache/restore@"),
            Self::TofuProvidersRestore => super::step_protocol::valid_provider_restore(kind),
            Self::TofuProvidersSave => super::step_protocol::valid_provider_save(kind),
            Self::MbxCache => valid_mbx_cache(kind),
            Self::MbxVersionCheck | Self::MbxWorkspaceCleanup => {
                matches!(kind, StepKind::Shell { .. })
            }
            Self::PreparePinnedTools
            | Self::PrepareRustComponents
            | Self::CargoDeny
            | Self::CargoMachete
            | Self::Actionlint
            | Self::Zizmor
            | Self::Markdownlint
            | Self::StrictJson
            | Self::FrontmatterId
            | Self::LinkCheck
            | Self::NativeValidators
            | Self::AcquireVelnor
            | Self::PreseedStage
            | Self::CheckGenerated
            | Self::PlanFormat
            | Self::MsrvQualification
            | Self::CargoSourcesFetch
            | Self::CacheKeys
            | Self::PreseedBuild
            | Self::PreseedManifest
            | Self::PreseedVerifyBuild
            | Self::PreseedVerifyManifest
            | Self::MbxPreflight => matches!(kind, StepKind::Shell { .. }),
        }
    }

    /// Output identity required for a role that publishes step outputs.
    pub(crate) const fn required_id(self) -> Option<StepId> {
        match self {
            Self::PlanProducer => Some(StepId::Plan),
            Self::BaselinePublisher => Some(StepId::PublishBaseline),
            Self::ToolsCacheIdentity => Some(StepId::ToolsCacheIdentity),
            Self::ToolsCacheRestore => Some(StepId::ToolsRestore),
            Self::NormalToolsCacheStage => Some(StepId::NormalToolsCacheStage),
            Self::NormalToolsCacheImport => Some(StepId::NormalToolsCacheImport),
            Self::NormalToolsCacheExport => Some(StepId::NormalToolsCacheExport),
            Self::CacheTarLaunchers => Some(StepId::CacheTarLaunchers),
            Self::PreseedStage => Some(StepId::PreseedStage),
            Self::TofuProvidersRestore => Some(StepId::TofuProviders),
            Self::MbxVersionCheck => Some(StepId::MbxReady),
            Self::CacheKeys => Some(StepId::CacheKeys),
            _ => None,
        }
    }

    /// Role required for a typed step output identity.
    pub(crate) const fn for_id(id: StepId) -> Self {
        match id {
            StepId::Plan => Self::PlanProducer,
            StepId::PublishBaseline => Self::BaselinePublisher,
            StepId::ToolsCacheIdentity => Self::ToolsCacheIdentity,
            StepId::NormalToolsCacheStage => Self::NormalToolsCacheStage,
            StepId::NormalToolsCacheImport => Self::NormalToolsCacheImport,
            StepId::NormalToolsCacheExport => Self::NormalToolsCacheExport,
            StepId::CacheTarLaunchers => Self::CacheTarLaunchers,
            StepId::PreseedStage => Self::PreseedStage,
            StepId::TofuProviders => Self::TofuProvidersRestore,
            StepId::MbxReady => Self::MbxVersionCheck,
            StepId::CacheKeys => Self::CacheKeys,
            StepId::SourcesRestore => Self::CargoSourcesRestore,
            StepId::SourcesFetch => Self::CargoSourcesFetch,
            StepId::ToolsRestore => Self::ToolsCacheRestore,
        }
    }
}

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
                format!("duplicate_step_id:{scope}:{}", step_id.as_str()),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "step_identity_tests.rs"]
mod tests;
