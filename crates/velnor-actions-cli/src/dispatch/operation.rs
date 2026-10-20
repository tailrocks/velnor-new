//! Internal operation and validated request types.

use std::path::PathBuf;

/// Private operation selected by the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InternalOp {
    /// Select the retained preview candidate for tap continuation.
    TapContinuation,
    /// Capture the independent review proposal.
    CaptureReviewCompletion,
    /// Publish the independent review result.
    EmitReviewCompletion,
    /// Admit immutable preview leaf artifacts.
    AcquirePreviewLeaves,
    /// Admit the complete preview aggregate.
    AdmitPreviewAggregate,
    /// Authenticate tap promotion inputs.
    TapAdmission,
    /// Materialize the request file from the GitHub environment.
    WriteRequest,
    /// Execute one named hosted platform check.
    ExecuteCheck,
    /// Run the credential-free native preview producer.
    ExecutePreviewProducer,
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
    /// Baseline-publish operation.
    Publish,
    /// Repository-maintenance operation behind its separate private gate.
    RepoPolicy,
    /// Read-only predecessor resolution for hosted qualification.
    ResolveQualification,
    /// Select one cache layer for the validated qualification plan.
    BindQualificationCache,
    /// Observe exact runtime identity for one generated cache owner.
    ObserveQualificationCacheRuntime,
    /// Observe one owner/layer's configured roots.
    ObserveQualificationCacheRoots,
    /// Persist one plan-bound physical owner/layer result.
    RecordQualificationCacheOwnerLayer,
    /// Combine all physical owner/layer records for one generated job.
    AggregateQualificationCacheOwner,
    /// Validate one restored cache against the backend census.
    ObserveQualificationCacheBackend,
    /// Import one admitted qualification cache payload.
    ImportQualificationCachePayload,
    /// Export one qualification cache payload to isolated staging.
    ExportQualificationCachePayload,
    /// Get the isolated staging path for the normal tools cache.
    NormalToolsCacheStagePath,
    /// Import an authenticated normal tools payload.
    ImportNormalToolsCachePayload,
    /// Export a normal tools payload to isolated staging.
    ExportNormalToolsCachePayload,
    /// Install verified tar and gtar aliases for cache action steps.
    InstallCacheTarLaunchers,
    /// Do a check of the normal `Tofu` backend and storage limits before archive download.
    PreflightNormalTofuCacheRestore,
    /// Record source and report observations for one Qualification lane.
    WriteQualificationSourceEvidence,
    /// Validate downloaded producer evidence and write the final receipt.
    WriteQualificationCacheReceipt,
    /// Fetch private source with the installation token.
    PrivateFetch,
    /// Revoke the installation token after the private fetch.
    PrivateRevoke,
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
    /// Isolated App-authenticated PR writer for a validated tap candidate.
    TapWriter,
    /// Isolated exact-head App-authenticated tap promotion merge.
    TapMerge,
}

/// Validated private request: operation plus exact request-file path.
#[derive(Debug)]
pub(crate) struct InternalRequest {
    /// Operation to run.
    pub(crate) op: InternalOp,
    /// Exact request-file path from the environment.
    pub(crate) path: PathBuf,
}
