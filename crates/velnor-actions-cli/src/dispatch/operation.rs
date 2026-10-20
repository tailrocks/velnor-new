//! Closed private operation identities; no public dispatch aliases.
use super::{
    ACQUIRE_RELEASE_LEAVES_OP, ADMIT_HOMEBREW_TAP_OP, ADMIT_RELEASE_AGGREGATE_OP,
    ADMIT_RELEASE_POLICY_OP, ADMIT_STABLE_TAG_ORIGIN_OP, BUN_REGISTRY_INSTALL_OP,
    CAPTURE_REVIEW_COMPLETION_OP, EMIT_REVIEW_COMPLETION_OP, EXECUTE_CHECK_OP,
    EXECUTE_RELEASE_PRODUCER_OP, FETCH_OP, MERGE_HOMEBREW_TAP_PR_OP, MERGE_OP, OP_ENV, PLAN_OP,
    PREPARE_HOMEBREW_TAP_OP, PRESEED_MANIFEST_OP, PRIVATE_FETCH_OP, PRIVATE_REVOKE_OP, PUBLISH_OP,
    PUBLISH_PACKAGE_RELEASE_OP, RECOVER_PACKAGE_RELEASE_OP, REPORT_OP,
    SELECT_RELEASE_TAP_CONTINUATION_OP, SELECT_STABLE_RELEASE_SOURCE_OP, SIGN_PACKAGE_RELEASE_OP,
    SOURCE_EVIDENCE_OP, TASK_EXECUTION_RESOLVER_OP, WRITE_HOMEBREW_TAP_PR_OP, WRITE_REQUEST_OP,
    WRITE_STABLE_TAG_INTENT_OP, WRITE_STABLE_TAG_ORIGIN_OP, cache_bind, cache_observe,
    cache_tar_launcher, env, normal_tofu_cache_preflight,
};

/// Private operation selected by the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InternalOp {
    TapContinuation,
    CaptureReviewCompletion,
    EmitReviewCompletion,
    TapAdmission,
    WriteRequest,
    ExecuteCheck,
    Plan,
    Merge,
    Fetch,
    Report,
    PreseedManifest,
    Publish,
    RepoPolicy,
    ResolveQualification,
    BindQualificationCache,
    ObserveQualificationCacheBackend,
    ImportQualificationCachePayload,
    ExportQualificationCachePayload,
    NormalToolsCacheStagePath,
    ImportNormalToolsCachePayload,
    ExportNormalToolsCachePayload,
    InstallCacheTarLaunchers,
    PreflightNormalTofuCacheRestore,
    WriteQualificationSourceEvidence,
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
    AcquireReleaseLeaves,
    AdmitReleaseAggregate,
    ReleasePolicyAdmission,
    StableSourceSelection,
    StableTagIntent,
    StableTagOrigin,
    StableTagAdmission,
    BunRegistryInstall,
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
        Ok(tag) if tag == PUBLISH_OP => InternalOp::Publish,
        Ok("repo-policy-v1") => InternalOp::RepoPolicy,
        Ok(tag) if crate::dispatch_qualification::is_resolver_op(tag) => {
            InternalOp::ResolveQualification
        }
        Ok(tag) if tag == cache_bind::OP => InternalOp::BindQualificationCache,
        Ok(tag) if tag == cache_observe::OP => InternalOp::ObserveQualificationCacheBackend,
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
        Ok(tag) if tag == TASK_EXECUTION_RESOLVER_OP => InternalOp::ResolveTaskExecution,
        Ok(tag) if tag == PRIVATE_FETCH_OP => InternalOp::PrivateFetch,
        Ok(tag) if tag == PRIVATE_REVOKE_OP => InternalOp::PrivateRevoke,
        Ok(tag) if tag == BUN_REGISTRY_INSTALL_OP => InternalOp::BunRegistryInstall,
        Ok(tag) if tag == SIGN_PACKAGE_RELEASE_OP => InternalOp::PackageSigner,
        Ok(tag) if tag == PUBLISH_PACKAGE_RELEASE_OP => InternalOp::PackagePublisher,
        Ok(tag) if tag == RECOVER_PACKAGE_RELEASE_OP => InternalOp::PackageRecovery,
        Ok(tag) if tag == PREPARE_HOMEBREW_TAP_OP => InternalOp::TapPrepare,
        Ok(tag) if tag == WRITE_HOMEBREW_TAP_PR_OP => InternalOp::TapWriter,
        Ok(tag) if tag == MERGE_HOMEBREW_TAP_PR_OP => InternalOp::TapMerge,
        Ok(tag) if tag == SELECT_STABLE_RELEASE_SOURCE_OP => InternalOp::StableSourceSelection,
        Ok(tag) if tag == ADMIT_RELEASE_POLICY_OP => InternalOp::ReleasePolicyAdmission,
        Ok(tag) if tag == WRITE_STABLE_TAG_INTENT_OP => InternalOp::StableTagIntent,
        Ok(tag) if tag == WRITE_STABLE_TAG_ORIGIN_OP => InternalOp::StableTagOrigin,
        Ok(tag) if tag == ADMIT_STABLE_TAG_ORIGIN_OP => InternalOp::StableTagAdmission,
        Ok(tag) if tag == EXECUTE_RELEASE_PRODUCER_OP => InternalOp::ExecuteReleaseProducer,
        Ok(tag) if tag == ACQUIRE_RELEASE_LEAVES_OP => InternalOp::AcquireReleaseLeaves,
        Ok(tag) if tag == ADMIT_RELEASE_AGGREGATE_OP => InternalOp::AdmitReleaseAggregate,
        _ => return None,
    })
}
