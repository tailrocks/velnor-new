//! Closed private operation identities; no public dispatch aliases.
use std::path::PathBuf;

use super::{
    ACQUIRE_PREVIEW_SOURCE_OP, ACQUIRE_RELEASE_LEAVES_OP, ADMIT_HOMEBREW_TAP_OP,
    ADMIT_RELEASE_AGGREGATE_OP, ADMIT_STABLE_TAG_ORIGIN_OP, BUN_ASSETS_EXPORT_OP,
    BUN_ASSETS_IMPORT_OP, BUN_ASSETS_PREPARE_OP, BUN_REGISTRY_INSTALL_OP, CACHE_RECEIPT_OP,
    CAPTURE_REVIEW_COMPLETION_OP, EMIT_REVIEW_COMPLETION_OP, EXECUTE_CHECK_OP,
    EXECUTE_RELEASE_PRODUCER_OP, FETCH_OP, FINALIZE_PREVIEW_SOURCE_OP, GRADLE_RUN_OP,
    MERGE_HOMEBREW_TAP_PR_OP, MERGE_OP, OP_ENV, PLAN_OP, PLAYWRIGHT_INSTALL_OP,
    PREPARE_HOMEBREW_TAP_OP, PRESEED_MANIFEST_OP, PRIVATE_FETCH_OP, PRIVATE_REVOKE_OP,
    PUBLISH_PACKAGE_RELEASE_OP, RECOVER_PACKAGE_RELEASE_OP, REPORT_OP, SELECT_PREVIEW_SOURCE_OP,
    SELECT_RELEASE_TAP_CONTINUATION_OP, SELECT_STABLE_RELEASE_SOURCE_OP, SIGN_PACKAGE_RELEASE_OP,
    SOURCE_EVIDENCE_OP, TASK_EXECUTION_RESOLVER_OP, WRITE_HOMEBREW_TAP_PR_OP, WRITE_REQUEST_OP,
    WRITE_STABLE_TAG_INTENT_OP, WRITE_STABLE_TAG_ORIGIN_OP, cache_bind, cache_owner_evidence,
    cache_runtime, cache_tar_launcher, env, normal_tofu_cache_preflight,
};

/// Private operation selected by the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InternalOp {
    TapContinuation,
    CaptureReviewCompletion,
    EmitReviewCompletion,
    TapAdmission,
    /// Validated Gradle execution in the runner workspace.
    GradleRun,
    WriteRequest,
    ExecuteCheck,
    Plan,
    Merge,
    Fetch,
    Report,
    PreseedManifest,
    RepoPolicy,
    ResolveQualification,
    BindQualificationCache,
    ObserveQualificationCacheRuntime,
    ObserveQualificationCacheRoots,
    RecordQualificationCacheOwnerLayer,
    AggregateQualificationCacheOwner,
    ImportQualificationCachePayload,
    ExportQualificationCachePayload,
    NormalToolsCacheStagePath,
    ImportNormalToolsCachePayload,
    ExportNormalToolsCachePayload,
    InstallCacheTarLaunchers,
    PreflightNormalTofuCacheRestore,
    WriteQualificationSourceEvidence,
    WriteQualificationCacheReceipt,
    ResolveTaskExecution,
    PackageSigner,
    PackagePublisher,
    PackageRecovery,
    TapPrepare,
    TapWriter,
    PrivateFetch,
    PrivateRevoke,
    TapMerge,
    ExecuteReleaseProducer,
    PreviewSourceSelection,
    PreviewSourceFinalization,
    PreviewSourceAcquisition,
    AcquireReleaseLeaves,
    AdmitReleaseAggregate,
    StableSourceSelection,
    StableTagIntent,
    StableTagOrigin,
    StableTagAdmission,
    BunRegistryInstall,
    BunAssetsExport,
    BunAssetsPrepare,
    BunAssetsImport,
    PlaywrightInstall,
}

/// Validated private request: operation plus exact request-file path.
#[derive(Debug)]
pub(super) struct InternalRequest {
    /// Operation to run.
    pub(super) op: InternalOp,
    /// Exact request-file path from the environment.
    pub(super) path: PathBuf,
}

pub(super) fn from_env() -> Option<InternalOp> {
    Some(match env::var(OP_ENV).as_deref() {
        Ok(tag) if tag == SELECT_RELEASE_TAP_CONTINUATION_OP => InternalOp::TapContinuation,
        Ok(tag) if tag == CAPTURE_REVIEW_COMPLETION_OP => InternalOp::CaptureReviewCompletion,
        Ok(tag) if tag == EMIT_REVIEW_COMPLETION_OP => InternalOp::EmitReviewCompletion,
        Ok(tag) if tag == ADMIT_HOMEBREW_TAP_OP => InternalOp::TapAdmission,
        Ok(tag) if tag == EXECUTE_CHECK_OP => InternalOp::ExecuteCheck,
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
        Ok(tag) if tag == cache_bind::OP => InternalOp::BindQualificationCache,
        Ok(tag) if tag == cache_runtime::RUNTIME_OP => InternalOp::ObserveQualificationCacheRuntime,
        Ok(tag) if tag == cache_runtime::ROOTS_OP => InternalOp::ObserveQualificationCacheRoots,
        Ok(tag) if tag == cache_owner_evidence::LAYER_OP => {
            InternalOp::RecordQualificationCacheOwnerLayer
        }
        Ok(tag) if tag == cache_owner_evidence::OWNER_OP => {
            InternalOp::AggregateQualificationCacheOwner
        }
        Ok(tag) if tag == cache_bind::transfer::IMPORT_OP => {
            InternalOp::ImportQualificationCachePayload
        }
        Ok(tag) if tag == cache_bind::transfer::EXPORT_OP => {
            InternalOp::ExportQualificationCachePayload
        }
        Ok(tag) if tag == cache_bind::normal_stage::OP => InternalOp::NormalToolsCacheStagePath,
        Ok(tag) if tag == cache_bind::transfer::normal::IMPORT_OP => {
            InternalOp::ImportNormalToolsCachePayload
        }
        Ok(tag) if tag == cache_bind::transfer::normal::EXPORT_OP => {
            InternalOp::ExportNormalToolsCachePayload
        }
        Ok(tag) if tag == cache_tar_launcher::OP => InternalOp::InstallCacheTarLaunchers,
        Ok(tag) if tag == normal_tofu_cache_preflight::OP => {
            InternalOp::PreflightNormalTofuCacheRestore
        }
        Ok(tag) if tag == SOURCE_EVIDENCE_OP => InternalOp::WriteQualificationSourceEvidence,
        Ok(tag) if tag == CACHE_RECEIPT_OP => InternalOp::WriteQualificationCacheReceipt,
        Ok(tag) if tag == TASK_EXECUTION_RESOLVER_OP => InternalOp::ResolveTaskExecution,
        Ok(tag) if tag == PRIVATE_FETCH_OP => InternalOp::PrivateFetch,
        Ok(tag) if tag == PRIVATE_REVOKE_OP => InternalOp::PrivateRevoke,
        Ok(tag) if tag == BUN_REGISTRY_INSTALL_OP => InternalOp::BunRegistryInstall,
        Ok(tag) if tag == BUN_ASSETS_EXPORT_OP => InternalOp::BunAssetsExport,
        Ok(tag) if tag == BUN_ASSETS_PREPARE_OP => InternalOp::BunAssetsPrepare,
        Ok(tag) if tag == BUN_ASSETS_IMPORT_OP => InternalOp::BunAssetsImport,
        Ok(tag) if tag == GRADLE_RUN_OP => InternalOp::GradleRun,
        Ok(tag) if tag == PLAYWRIGHT_INSTALL_OP => InternalOp::PlaywrightInstall,
        Ok(tag) if tag == SIGN_PACKAGE_RELEASE_OP => InternalOp::PackageSigner,
        Ok(tag) if tag == PUBLISH_PACKAGE_RELEASE_OP => InternalOp::PackagePublisher,
        Ok(tag) if tag == RECOVER_PACKAGE_RELEASE_OP => InternalOp::PackageRecovery,
        Ok(tag) if tag == PREPARE_HOMEBREW_TAP_OP => InternalOp::TapPrepare,
        Ok(tag) if tag == WRITE_HOMEBREW_TAP_PR_OP => InternalOp::TapWriter,
        Ok(tag) if tag == MERGE_HOMEBREW_TAP_PR_OP => InternalOp::TapMerge,
        Ok(tag) if tag == SELECT_STABLE_RELEASE_SOURCE_OP => InternalOp::StableSourceSelection,
        Ok(tag) if tag == SELECT_PREVIEW_SOURCE_OP => InternalOp::PreviewSourceSelection,
        Ok(tag) if tag == FINALIZE_PREVIEW_SOURCE_OP => InternalOp::PreviewSourceFinalization,
        Ok(tag) if tag == ACQUIRE_PREVIEW_SOURCE_OP => InternalOp::PreviewSourceAcquisition,
        Ok(tag) if tag == WRITE_STABLE_TAG_INTENT_OP => InternalOp::StableTagIntent,
        Ok(tag) if tag == WRITE_STABLE_TAG_ORIGIN_OP => InternalOp::StableTagOrigin,
        Ok(tag) if tag == ADMIT_STABLE_TAG_ORIGIN_OP => InternalOp::StableTagAdmission,
        Ok(tag) if tag == EXECUTE_RELEASE_PRODUCER_OP => InternalOp::ExecuteReleaseProducer,
        Ok(tag) if tag == ACQUIRE_RELEASE_LEAVES_OP => InternalOp::AcquireReleaseLeaves,
        Ok(tag) if tag == ADMIT_RELEASE_AGGREGATE_OP => InternalOp::AdmitReleaseAggregate,
        _ => return None,
    })
}
