// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::cache::{CACHE_CLEANUP_OP, CACHE_PUBLISH_OP, CACHE_RECOVER_OP};
use super::{
    ACQUIRE_PREVIEW_SOURCE_OP, ACQUIRE_RELEASE_LEAVES_OP, ADMIT_HOMEBREW_TAP_OP,
    ADMIT_RELEASE_AGGREGATE_OP, ADMIT_STABLE_TAG_ORIGIN_OP, ASSEMBLE_GROUP_LEDGERS_OP,
    BUN_ASSETS_EXPORT_OP, BUN_ASSETS_IMPORT_OP, BUN_ASSETS_PREPARE_OP,
    BUN_ASSETS_REPOSITORY_CHECK_OP, BUN_REGISTRY_INSTALL_OP, CAPTURE_REVIEW_COMPLETION_OP,
    EMIT_GROUP_LEDGER_OP, EMIT_REVIEW_COMPLETION_OP, EXECUTE_CHECK_OP, EXECUTE_RELEASE_PRODUCER_OP,
    FETCH_OP, FINALIZE_PREVIEW_SOURCE_OP, GITHUB_EVIDENCE_FETCH_OP, GITHUB_EVIDENCE_IMPORT_OP,
    GRADLE_RUN_OP, MERGE_HOMEBREW_TAP_PR_OP, MERGE_OP, OP_ENV, PLAN_OP, PLAYWRIGHT_INSTALL_OP,
    PREPARE_HOMEBREW_TAP_OP, PRESEED_MANIFEST_OP, PRIVATE_FETCH_OP, PRIVATE_REVOKE_OP,
    PUBLISH_PACKAGE_RELEASE_OP, RECOVER_PACKAGE_RELEASE_OP, REPORT_OP, SELECT_PREVIEW_SOURCE_OP,
    SELECT_RELEASE_TAP_CONTINUATION_OP, SELECT_STABLE_RELEASE_SOURCE_OP, SIGN_PACKAGE_RELEASE_OP,
    TASK_EXECUTION_RESOLVER_OP, WRITE_HOMEBREW_TAP_PR_OP, WRITE_REQUEST_OP,
    WRITE_STABLE_TAG_INTENT_OP, WRITE_STABLE_TAG_ORIGIN_OP, env,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InternalOp {
    ExecuteNativeHomebrewCheck,

    QualifyHomebrew,

    SeedParallelTargets,

    TapContinuation,

    CaptureReviewCompletion,

    EmitReviewCompletion,

    WriteRequest,

    ExecuteCheck,

    ExecuteReleaseProducer,

    AcquireReleaseLeaves,

    AdmitReleaseAggregate,

    PreviewSourceSelection,

    PreviewSourceFinalization,

    PreviewSourceAcquisition,

    StableSourceSelection,

    StableTagIntent,

    StableTagOrigin,

    StableTagAdmission,

    Plan,

    Merge,

    Fetch,

    Report,

    PreseedManifest,

    RepoPolicy,

    ResolveQualification,

    ResolveTaskExecution,

    PackageSigner,

    PackagePublisher,

    PackageRecovery,

    TapPrepare,

    TapAdmission,

    PrivateFetch,

    PrivateRevoke,

    GradleRun,

    BunRegistryInstall,

    BunAssetsRepositoryCheck,

    BunAssetsExport,

    BunAssetsPrepare,

    BunAssetsImport,

    GitHubEvidenceFetch,

    GitHubEvidenceImport,

    PlaywrightInstall,

    TapWriter,

    TapMerge,

    CachePublish,

    CacheRecover,

    CacheCleanup,

    EmitGroupLedger,

    AssembleGroupLedgers,
}

pub(super) fn from_env() -> Option<InternalOp> {
    Some(match env::var(OP_ENV).as_deref() {
        Ok("execute-native-homebrew-check-v1") => InternalOp::ExecuteNativeHomebrewCheck,
        Ok("seed-parallel-targets-v1") => InternalOp::SeedParallelTargets,
        Ok("qualify-homebrew-v1") => InternalOp::QualifyHomebrew,
        Ok(tag) if tag == SELECT_RELEASE_TAP_CONTINUATION_OP => InternalOp::TapContinuation,
        Ok(tag) if tag == CAPTURE_REVIEW_COMPLETION_OP => InternalOp::CaptureReviewCompletion,
        Ok(tag) if tag == EMIT_REVIEW_COMPLETION_OP => InternalOp::EmitReviewCompletion,
        Ok(tag) if tag == EXECUTE_CHECK_OP => InternalOp::ExecuteCheck,
        Ok(tag) if tag == SELECT_STABLE_RELEASE_SOURCE_OP => InternalOp::StableSourceSelection,
        Ok(tag) if tag == SELECT_PREVIEW_SOURCE_OP => InternalOp::PreviewSourceSelection,
        Ok(tag) if tag == FINALIZE_PREVIEW_SOURCE_OP => InternalOp::PreviewSourceFinalization,
        Ok(tag) if tag == ACQUIRE_PREVIEW_SOURCE_OP => InternalOp::PreviewSourceAcquisition,
        Ok(tag) if tag == ADMIT_STABLE_TAG_ORIGIN_OP => InternalOp::StableTagAdmission,
        Ok(tag) if tag == WRITE_STABLE_TAG_INTENT_OP => InternalOp::StableTagIntent,
        Ok(tag) if tag == WRITE_STABLE_TAG_ORIGIN_OP => InternalOp::StableTagOrigin,
        Ok(tag) if tag == ACQUIRE_RELEASE_LEAVES_OP => InternalOp::AcquireReleaseLeaves,
        Ok(tag) if tag == EXECUTE_RELEASE_PRODUCER_OP => InternalOp::ExecuteReleaseProducer,
        Ok(tag) if tag == ADMIT_RELEASE_AGGREGATE_OP => InternalOp::AdmitReleaseAggregate,
        Ok(tag) if tag == WRITE_REQUEST_OP => InternalOp::WriteRequest,
        Ok(tag) if tag == PLAN_OP => InternalOp::Plan,
        Ok(tag) if tag == MERGE_OP => InternalOp::Merge,
        Ok(tag) if tag == FETCH_OP => InternalOp::Fetch,
        Ok(tag) if tag == REPORT_OP => InternalOp::Report,
        Ok(tag) if tag == PRESEED_MANIFEST_OP => InternalOp::PreseedManifest,
        Ok("repo-policy-v1") => InternalOp::RepoPolicy,
        Ok(tag) if crate::dispatch_qualification::is_resolver_op(tag) => {
            InternalOp::ResolveQualification
        }
        Ok(tag) if tag == TASK_EXECUTION_RESOLVER_OP => InternalOp::ResolveTaskExecution,
        Ok(tag) if tag == PRIVATE_FETCH_OP => InternalOp::PrivateFetch,
        Ok(tag) if tag == PRIVATE_REVOKE_OP => InternalOp::PrivateRevoke,
        Ok(tag) if tag == BUN_REGISTRY_INSTALL_OP => InternalOp::BunRegistryInstall,
        Ok(tag) if tag == BUN_ASSETS_REPOSITORY_CHECK_OP => InternalOp::BunAssetsRepositoryCheck,
        Ok(tag) if tag == BUN_ASSETS_EXPORT_OP => InternalOp::BunAssetsExport,
        Ok(tag) if tag == BUN_ASSETS_PREPARE_OP => InternalOp::BunAssetsPrepare,
        Ok(tag) if tag == BUN_ASSETS_IMPORT_OP => InternalOp::BunAssetsImport,
        Ok(tag) if tag == GITHUB_EVIDENCE_FETCH_OP => InternalOp::GitHubEvidenceFetch,
        Ok(tag) if tag == GITHUB_EVIDENCE_IMPORT_OP => InternalOp::GitHubEvidenceImport,
        Ok(tag) if tag == GRADLE_RUN_OP => InternalOp::GradleRun,
        Ok(tag) if tag == PLAYWRIGHT_INSTALL_OP => InternalOp::PlaywrightInstall,
        Ok(tag) if tag == SIGN_PACKAGE_RELEASE_OP => InternalOp::PackageSigner,
        Ok(tag) if tag == PUBLISH_PACKAGE_RELEASE_OP => InternalOp::PackagePublisher,
        Ok(tag) if tag == RECOVER_PACKAGE_RELEASE_OP => InternalOp::PackageRecovery,
        Ok(tag) if tag == PREPARE_HOMEBREW_TAP_OP => InternalOp::TapPrepare,
        Ok(tag) if tag == ADMIT_HOMEBREW_TAP_OP => InternalOp::TapAdmission,
        Ok(tag) if tag == WRITE_HOMEBREW_TAP_PR_OP => InternalOp::TapWriter,
        Ok(tag) if tag == MERGE_HOMEBREW_TAP_PR_OP => InternalOp::TapMerge,
        Ok(tag) if tag == CACHE_PUBLISH_OP => InternalOp::CachePublish,
        Ok(tag) if tag == CACHE_RECOVER_OP => InternalOp::CacheRecover,
        Ok(tag) if tag == CACHE_CLEANUP_OP => InternalOp::CacheCleanup,
        Ok(tag) if tag == EMIT_GROUP_LEDGER_OP => InternalOp::EmitGroupLedger,
        Ok(tag) if tag == ASSEMBLE_GROUP_LEDGERS_OP => InternalOp::AssembleGroupLedgers,
        _ => return None,
    })
}
