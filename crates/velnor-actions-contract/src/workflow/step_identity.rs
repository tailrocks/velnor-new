// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::step::{Step, StepKind};
pub use super::step_payloads::{TOOLS_RESTORE_LOOKUP_ONLY_INPUT, ToolsRestoreForm};
use super::step_payloads::{valid_checkout_payload, valid_mise_setup, valid_tool_seed_payload};
pub(super) use super::step_payloads::{valid_tools_cache_identity, valid_tools_cache_scrub};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

pub const FIXTURE_CHECKOUT_BLOB_FILTER: &str = "blob:limit=18446744073709551615";

#[path = "step_identity_payload.rs"]
mod payload;
#[path = "step_identity_sequence.rs"]
mod sequence;
#[path = "step_identity_shell_roles.rs"]
mod shell_roles;
use payload::{
    action_has_prefix_for_kind, internal_operation, valid_bun_assets_handoff,
    valid_jactionlint_verifier_payload, valid_rust_verifier_payload,
};
pub use payload::{is_configured_checkout, is_tool_seed_step};
pub use sequence::{validate_step_identity_scope, validate_step_sequence};

pub const TOFU_PROVIDER_ADMISSION_USES: &str = "./.github/actions/tofu-provider-admission";

pub const TOOL_SEED_USES: &str = "./.github/actions/velnor-tool-seed";

pub const TOOLS_CACHE_RESTORE_USES: &str = "./.github/actions/velnor-tools-cache-restore";

pub const TOOLS_CACHE_SCRUB_USES: &str = "./.github/actions/velnor-tools-cache-scrub";

pub const TOOLS_CACHE_RESTORE_ACTION_USES: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

pub const TOOLS_CACHE_V2_KEY_EXPRESSION: &str = "mise-tools-v2-${{steps.v2.outputs.identity}}";

pub const TOOLS_CACHE_PATHS: [&str; 5] = [
    "~/.local/share/mise",
    super::cache_family::RUSTUP_HOME_EXPR,
    "${{ runner.temp }}/velnor/cargo/.crates.toml",
    "${{ runner.temp }}/velnor/cargo/.crates2.json",
    "${{ runner.temp }}/velnor/cargo/bin",
];

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
    ReleaseBudget,
    ReleaseAdmission,
    ReleaseVerification,
    ReleaseRustUpload,
    ReleaseReceiptUpload,
    ReleasePrCandidate,
    ReleasePrArtifact,
    ReleaseRustArchive,
    AcquireReleasePlz,
    AcquireCargoSemverChecks,
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

    ToolsCacheScrub,

    CargoSourcesRestore,

    CargoSourcesSave,

    CargoSourcesFetch,

    CacheKeys,

    TofuProvidersSave,

    TofuProvidersRestore,

    TofuProvidersPrepare,

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

    MbxBundleExport,

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

    LedgerCliBuild,

    LedgerCliPublish,

    LedgerCliUpload,

    LedgerCliDownload,

    LedgerCliVerify,

    StateSelect,

    StateSealUpload,

    StateSealDownload,

    StateSealSubject,

    StateSealBind,

    StateSealTip,

    StateSealAttest,

    StateSealSave,

    StateSealAdmit,

    StateSealRetire,

    StateProduced,

    StateSkeleton,

    StateSkeletonRestore,
    NonInputGuard,
    MbxStats,
}

#[path = "step_identity_role.rs"]
mod role_impl;
