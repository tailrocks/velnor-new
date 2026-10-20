//! Closed private operation identities; no public dispatch aliases.
use super::{
    ACQUIRE_RELEASE_LEAVES_OP, ADMIT_HOMEBREW_TAP_OP, ADMIT_RELEASE_AGGREGATE_OP,
    ADMIT_RELEASE_POLICY_OP, ADMIT_STABLE_TAG_ORIGIN_OP, BUN_REGISTRY_INSTALL_OP,
    CAPTURE_REVIEW_COMPLETION_OP, EMIT_REVIEW_COMPLETION_OP, EXECUTE_CHECK_OP,
    EXECUTE_RELEASE_PRODUCER_OP, FETCH_OP, GRADLE_RUN_OP, MERGE_HOMEBREW_TAP_PR_OP, MERGE_OP,
    OP_ENV, PLAN_OP, PLAYWRIGHT_INSTALL_OP, PREPARE_HOMEBREW_TAP_OP, PRESEED_MANIFEST_OP,
    PRIVATE_FETCH_OP, PRIVATE_REVOKE_OP, PUBLISH_PACKAGE_RELEASE_OP, RECOVER_PACKAGE_RELEASE_OP,
    REPORT_OP, SELECT_RELEASE_TAP_CONTINUATION_OP, SELECT_STABLE_RELEASE_SOURCE_OP,
    SIGN_PACKAGE_RELEASE_OP, TASK_EXECUTION_RESOLVER_OP, WRITE_HOMEBREW_TAP_PR_OP,
    WRITE_REQUEST_OP, WRITE_STABLE_TAG_INTENT_OP, WRITE_STABLE_TAG_ORIGIN_OP, env,
};

/// Private operation selected by the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InternalOp {
    /// Source-free authenticated selection of the retained tap candidate.
    TapContinuation,
    /// Read-only authenticated review proposal capture.
    CaptureReviewCompletion,
    /// Independent C-App exact-head check emission.
    EmitReviewCompletion,
    /// Materialize the request file from the GitHub environment.
    WriteRequest,
    /// Execute one named hosted platform check.
    ExecuteCheck,
    /// Run the credential-free native preview producer.
    ExecuteReleaseProducer,
    /// Source-free credentialed leaf acquisition before repository checkout.
    AcquireReleaseLeaves,
    /// Source-free authentication of the complete native preview union.
    AdmitReleaseAggregate,
    /// Resolve and admit the static release policy before source checkout.
    ReleasePolicyAdmission,
    /// Read-only selection of fresh or retained stable source authority.
    StableSourceSelection,
    /// Source-free stable pre-tag authorization.
    StableTagIntent,
    /// Bind the actual permanent tag to its retained native intent.
    StableTagOrigin,
    /// Authenticate retained tag authority before source checkout.
    StableTagAdmission,
    /// Plan operation.
    Plan,
    /// Merge operation.
    Merge,
    /// Matrix-report fetch operation.
    Fetch,
    /// Task-report production operation.
    Report,
    /// Pre-seed manifest-writing operation.
    PreseedManifest,
    /// Repository-maintenance operation behind its separate private gate.
    RepoPolicy,
    /// Read-only predecessor resolution for hosted qualification.
    ResolveQualification,
    /// Data-only task execution record resolution for a generated wrapper.
    ResolveTaskExecution,
    /// Isolated, credentialed signing of a plan-bound package release.
    PackageSigner,
    /// Isolated publisher for an authenticated signed native package.
    PackagePublisher,
    /// Authenticated replay of a retained signed preview candidate.
    PackageRecovery,
    /// Credential-free updater prepared from captured public tap main.
    TapPrepare,
    /// Source-free authentication before credential-free tap preparation.
    TapAdmission,
    /// Token-bound fetch of private cross-repository sources (HD-7).
    PrivateFetch,
    /// Installation-token revocation after the private fetch (HD-7).
    PrivateRevoke,
    /// `gradle-run-v1`: verify the wrapper, install the JDK, run Gradle.
    GradleRun,
    /// Registry phase of `bun install` under the job token (HD-6, HD-7).
    BunRegistryInstall,
    /// Hash-verified Playwright browser install (HD-6).
    PlaywrightInstall,
    /// Isolated App-authenticated PR writer for a validated tap candidate.
    TapWriter,
    /// Isolated exact-head App-authenticated tap promotion merge.
    TapMerge,
}

pub(super) fn from_env() -> Option<InternalOp> {
    Some(match env::var(OP_ENV).as_deref() {
        Ok(tag) if tag == SELECT_RELEASE_TAP_CONTINUATION_OP => InternalOp::TapContinuation,
        Ok(tag) if tag == CAPTURE_REVIEW_COMPLETION_OP => InternalOp::CaptureReviewCompletion,
        Ok(tag) if tag == EMIT_REVIEW_COMPLETION_OP => InternalOp::EmitReviewCompletion,
        Ok(tag) if tag == EXECUTE_CHECK_OP => InternalOp::ExecuteCheck,
        Ok(tag) if tag == SELECT_STABLE_RELEASE_SOURCE_OP => InternalOp::StableSourceSelection,
        Ok(tag) if tag == ADMIT_RELEASE_POLICY_OP => InternalOp::ReleasePolicyAdmission,
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
        Ok(tag) if tag == GRADLE_RUN_OP => InternalOp::GradleRun,
        Ok(tag) if tag == PLAYWRIGHT_INSTALL_OP => InternalOp::PlaywrightInstall,
        Ok(tag) if tag == SIGN_PACKAGE_RELEASE_OP => InternalOp::PackageSigner,
        Ok(tag) if tag == PUBLISH_PACKAGE_RELEASE_OP => InternalOp::PackagePublisher,
        Ok(tag) if tag == RECOVER_PACKAGE_RELEASE_OP => InternalOp::PackageRecovery,
        Ok(tag) if tag == PREPARE_HOMEBREW_TAP_OP => InternalOp::TapPrepare,
        Ok(tag) if tag == ADMIT_HOMEBREW_TAP_OP => InternalOp::TapAdmission,
        Ok(tag) if tag == WRITE_HOMEBREW_TAP_PR_OP => InternalOp::TapWriter,
        Ok(tag) if tag == MERGE_HOMEBREW_TAP_PR_OP => InternalOp::TapMerge,
        _ => return None,
    })
}
