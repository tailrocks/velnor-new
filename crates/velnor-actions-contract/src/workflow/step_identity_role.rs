// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

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

    PrepareRuntimeTools,

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

    AcquireProtectedPreview,

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
