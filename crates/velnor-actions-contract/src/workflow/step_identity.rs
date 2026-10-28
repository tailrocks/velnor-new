// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::step::{Step, StepKind};
pub(super) use super::step_payloads::valid_tools_cache_identity;
use super::step_payloads::{valid_checkout_payload, valid_mise_setup, valid_tool_seed_payload};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[path = "step_identity_payload.rs"]
mod payload;
use payload::{
    action_has_prefix_for_kind, internal_operation, valid_jactionlint_verifier_payload,
    valid_rust_verifier_payload,
};
pub use payload::{is_configured_checkout, is_tool_seed_step};

pub const TOFU_PROVIDER_ADMISSION_USES: &str = "./.github/actions/tofu-provider-admission";

pub const TOOL_SEED_USES: &str = "./.github/actions/velnor-tool-seed";

pub const TOOLS_CACHE_RESTORE_USES: &str = "./.github/actions/velnor-tools-cache-restore";

pub const TOOLS_CACHE_PRELUDE_USES: [&str; 3] = [
    "./.github/actions/velnor-tools-prelude-u22",
    "./.github/actions/velnor-tools-prelude-u24",
    "./.github/actions/velnor-tools-prelude-u26",
];

pub const RUST_VERIFIER_USES: [&str; 2] = [
    "./.github/actions/velnor-verify-restored-rust",
    "./.github/actions/velnor-verify-rust-manifest",
];

pub const JACTIONLINT_VERIFIER_USES: &str = "./.github/actions/velnor-verify-jactionlint";

pub(super) fn verifier_action_role(uses: &str) -> Option<StepRole> {
    if RUST_VERIFIER_USES.contains(&uses) {
        Some(StepRole::VerifyRustManifest)
    } else if uses == JACTIONLINT_VERIFIER_USES {
        Some(StepRole::VerifyJactionlint)
    } else {
        None
    }
}

pub const TOOLS_CACHE_IDENTITY_DIGEST_INPUT: &str = "d";

pub const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = super::cache_family::TOFU_PROVIDER_CACHE_BASE_EXPR;

pub const TOFU_PROVIDERS_KEY_OUTPUT_EXPR: &str = "${{ steps.tofu-providers.outputs.cache-key }}";

pub const TOFU_PROVIDERS_PATH_OUTPUT_EXPR: &str = "${{ steps.tofu-providers.outputs.cache-path }}";

pub const MBX_WORKSPACE_CLEAN_CONDITION: &str = "always() && steps.mbx-ready.outcome == 'success'";
pub use super::step_private::{
    BUN_ASSETS_EXPORT_OPERATION, BUN_ASSETS_IMPORT_OPERATION, BUN_ASSETS_PREPARE_OPERATION,
    BUN_ASSETS_REPOSITORY_CHECK_OPERATION, BUN_REGISTRY_INSTALL_OPERATION,
    GITHUB_EVIDENCE_FETCH_OPERATION, GITHUB_EVIDENCE_IMPORT_OPERATION, GITHUB_EVIDENCE_SPEC_ENV,
    GITHUB_EVIDENCE_TOKEN_ENV, PRIVATE_FETCH_OPERATION, PRIVATE_FETCH_TOKEN_ENV,
    PRIVATE_NPM_READ_EXPR, PRIVATE_NPM_READ_TOKEN_ENV, PRIVATE_REVOKE_OPERATION,
    PRIVATE_TOKEN_MINT_ACTION_PREFIX, PRIVATE_TOKEN_OUTPUT_EXPR,
};

pub use super::step_id::StepId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepRole {
    Checkout,

    ToolSeed,

    PreparePinnedTools,

    PrepareRustComponents,

    VerifyRustManifest,

    VerifyJactionlint,

    MiseSetup,

    CargoDeny,

    CargoMachete,

    Jactionlint,

    Markdownlint,

    StrictJson,

    FrontmatterId,

    LinkCheck,

    NativeValidators,

    PlanProducer,

    BuildTaskRun,

    AcquireVelnor,

    PreseedStage,

    DownloadPlan,

    CheckGenerated,

    PublishPlan,

    MatrixReportUpload,

    AttestationDownload,

    FetchReports,

    PublishFinal,

    PlanFormat,

    MsrvQualification,

    ToolsCacheSave,

    ToolsCacheIdentity,

    ToolsCacheRestore,

    CargoSourcesRestore,

    CargoSourcesSave,

    CargoSourcesFetch,

    CacheKeys,

    TofuProvidersSave,

    TofuProvidersRestore,

    TofuProviderUse,

    PreseedBuild,

    PreseedManifest,

    PreseedVerifyBuild,

    PreseedUpload,

    PreseedDownload,

    PreseedVerifyManifest,

    MbxCache,

    MbxVersionCheck,

    MbxPreflight,

    MbxWorkspaceCleanup,

    MbxBundleKey,

    MbxBundleRestore,

    MbxBundleImport,

    MbxProducerMark,

    MbxSeedBuild,

    MbxBundleExport,

    MbxBundleSave,

    MbxBundleRecord,

    FamilyKeys,

    FamilyRestore,

    FamilySizeCheck,

    FamilySave,

    PrivateTokenMint,

    PrivateFetch,

    PrivateRevoke,

    BunRegistryInstall,

    BunAssetsHandoff,

    PlaywrightInstall,

    GitHubEvidenceFetch,

    GitHubEvidenceImport,
}

fn valid_bun_assets_handoff(kind: &StepKind) -> bool {
    [
        BUN_ASSETS_EXPORT_OPERATION,
        BUN_ASSETS_PREPARE_OPERATION,
        BUN_ASSETS_IMPORT_OPERATION,
        BUN_ASSETS_REPOSITORY_CHECK_OPERATION,
    ]
    .into_iter()
    .any(|operation| internal_operation(kind, operation))
}

impl StepRole {
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

    fn matches_payload(self, kind: &StepKind) -> bool {
        match self {
            Self::Checkout => valid_checkout_payload(kind),
            Self::ToolSeed => valid_tool_seed_payload(kind),
            Self::MiseSetup => valid_mise_setup(kind),
            Self::TofuProviderUse => super::step_protocol::valid_provider_use(kind),
            Self::PlanProducer => internal_operation(kind, "plan-v1"),
            Self::FetchReports => internal_operation(kind, "fetch-reports-v1"),
            Self::DownloadPlan | Self::AttestationDownload | Self::PreseedDownload => {
                action_has_prefix_for_kind(kind, "actions/download-artifact@")
            }
            Self::PublishPlan
            | Self::MatrixReportUpload
            | Self::PublishFinal
            | Self::PreseedUpload => action_has_prefix_for_kind(kind, "actions/upload-artifact@"),
            Self::ToolsCacheSave
            | Self::CargoSourcesSave
            | Self::MbxBundleSave
            | Self::FamilySave => action_has_prefix_for_kind(kind, "actions/cache/save@"),
            Self::ToolsCacheIdentity => valid_tools_cache_identity(kind),
            Self::ToolsCacheRestore => matches!(
                kind,
                StepKind::Action { uses, .. } if uses == TOOLS_CACHE_RESTORE_USES
            ),
            Self::CargoSourcesRestore | Self::MbxBundleRestore | Self::FamilyRestore => {
                action_has_prefix_for_kind(kind, "actions/cache/restore@")
            }
            Self::TofuProvidersRestore => super::step_protocol::valid_provider_restore(kind),
            Self::TofuProvidersSave => super::step_protocol::valid_provider_save(kind),
            Self::PrivateTokenMint => matches!(kind,
                StepKind::Action { uses, .. } if uses.starts_with(PRIVATE_TOKEN_MINT_ACTION_PREFIX)),
            Self::PrivateFetch => internal_operation(kind, PRIVATE_FETCH_OPERATION),
            Self::GitHubEvidenceFetch => internal_operation(kind, GITHUB_EVIDENCE_FETCH_OPERATION),
            Self::GitHubEvidenceImport => {
                internal_operation(kind, GITHUB_EVIDENCE_IMPORT_OPERATION)
            }
            Self::PrivateRevoke => internal_operation(kind, PRIVATE_REVOKE_OPERATION),
            Self::BunRegistryInstall => internal_operation(kind, BUN_REGISTRY_INSTALL_OPERATION),
            Self::BunAssetsHandoff => valid_bun_assets_handoff(kind),
            Self::MbxCache => super::step_mbx_lifecycle::valid_mbx_cache(kind),
            Self::PlaywrightInstall => super::step_protocol::valid_playwright_install(kind),
            Self::VerifyRustManifest => valid_rust_verifier_payload(kind),
            Self::VerifyJactionlint => valid_jactionlint_verifier_payload(kind),
            Self::PreparePinnedTools
            | Self::MbxVersionCheck
            | Self::MbxWorkspaceCleanup
            | Self::MbxBundleKey
            | Self::FamilyKeys
            | Self::FamilySizeCheck
            | Self::MbxBundleImport
            | Self::MbxProducerMark
            | Self::MbxSeedBuild
            | Self::MbxBundleExport
            | Self::MbxBundleRecord
            | Self::PrepareRustComponents
            | Self::CargoDeny
            | Self::CargoMachete
            | Self::Jactionlint
            | Self::Markdownlint
            | Self::StrictJson
            | Self::FrontmatterId
            | Self::LinkCheck
            | Self::NativeValidators
            | Self::AcquireVelnor
            | Self::BuildTaskRun
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

    pub(crate) const fn required_id(self) -> Option<StepId> {
        match self {
            Self::PlanProducer => Some(StepId::Plan),
            Self::BuildTaskRun => Some(StepId::BuildTaskRun),
            Self::ToolsCacheIdentity => Some(StepId::ToolsCacheIdentity),
            Self::TofuProvidersRestore => Some(StepId::TofuProviders),
            Self::MbxVersionCheck => Some(StepId::MbxReady),
            Self::CacheKeys => Some(StepId::CacheKeys),
            Self::ToolsCacheRestore => Some(StepId::ToolsRestore),
            Self::PrivateTokenMint => Some(StepId::DepsToken),
            Self::MbxBundleKey => Some(StepId::MbxBundleKey),
            // No pinned id: the writer's queue-slot recheck re-runs the
            // bundle restore without an id (a job carries one canonical
            // restore id), like the other restore roles below.
            Self::MbxBundleExport => Some(StepId::MbxBundleExport),
            Self::FamilyKeys => Some(StepId::FamilyKeys),
            _ => None,
        }
    }

    pub(crate) const fn for_id(id: StepId) -> Self {
        match id {
            StepId::Plan => Self::PlanProducer,
            StepId::BuildTaskRun => Self::BuildTaskRun,
            StepId::ToolsCacheIdentity => Self::ToolsCacheIdentity,
            StepId::TofuProviders => Self::TofuProvidersRestore,
            StepId::MbxReady => Self::MbxVersionCheck,
            StepId::CacheKeys => Self::CacheKeys,
            StepId::SourcesRestore => Self::CargoSourcesRestore,
            StepId::SourcesFetch => Self::CargoSourcesFetch,
            StepId::ToolsRestore => Self::ToolsCacheRestore,
            StepId::DepsToken => Self::PrivateTokenMint,
            StepId::MbxBundleKey => Self::MbxBundleKey,
            StepId::MbxBundleRestore => Self::MbxBundleRestore,
            StepId::MbxBundleExport => Self::MbxBundleExport,
            StepId::FamilyKeys => Self::FamilyKeys,
            StepId::FamilySizeBunInstall
            | StepId::FamilySizePlaywrightBrowsers
            | StepId::FamilySizeDbtPackages
            | StepId::FamilySizePythonVenv
            | StepId::FamilySizeAnsibleCollections => Self::FamilySizeCheck,
        }
    }
}

pub fn validate_step_sequence(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    validate_step_identities(steps, scope)?;
    super::step_mbx_lifecycle::validate_mbx_cleanup_sequence(steps, scope)?;
    super::step_protocol::validate_tofu_provider_sequence(steps, scope)
}

pub fn validate_step_identity_scope(steps: &[Step], scope: &str) -> Result<(), ContractError> {
    validate_step_identities(steps, scope)
}

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
