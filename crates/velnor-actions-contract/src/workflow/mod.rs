//! Stack-neutral workflow IR, matrix, plan, and report types.
pub mod action_report;
pub mod artifacts;
pub mod baseline;
pub mod cache_ids;
pub mod cache_mode;
pub mod cache_receipt_recipe;
pub mod cache_snapshot_domain;
pub mod cache_trust;
pub mod crate_job;
pub mod dispatch;
pub mod execute;
pub mod helper_obligation;
pub mod helper_obligation_descriptor;
pub mod ir;
pub mod jobs;
pub mod mbx_export_descriptor;
pub mod mbx_producer;
pub mod native_publish;
pub mod native_tools;
pub mod native_validation_descriptor;
pub mod needs;
pub mod observer;
pub mod outputs;
pub mod pages;
pub mod permissions;
pub mod plan;
pub mod producer_inventory;
pub mod qualification;
pub mod report;
pub mod scope;
pub mod source_helper;
pub mod source_producer;
pub mod step;
pub mod task_execution_identity;
pub mod timeout;
pub mod timing;
pub mod tool_producer;
pub mod tool_producer_selection;
pub mod trust;
pub use action_report::{ActionBinding, ActionOutcome, ActionReport};
pub use artifacts::{
    CANDIDATE_ATTESTATION_FILENAME, CANDIDATE_EVIDENCE_SUBDIR, FINAL_JSON_FILENAME,
    MATRIX_JSON_FILENAME, PLAN_JSON_FILENAME, check_matrix_agreement, matrix_json_bytes,
    plan_json_bytes,
};
pub use baseline::{BaselineProof, BaselineStatus, ManifestTaskProof, PlanBaseline};
pub use cache_ids::EntryCacheIds;
pub use cache_mode::CacheMode;
pub use cache_receipt_recipe::{cache_producer_recipe_digest, cache_receipt_root};
pub use cache_snapshot_domain::CacheSnapshotDomain;
pub use crate_job::{CrateJob, CrateObligation};
pub use execute::{ExecuteTaskIds, ExecuteTaskRef};
pub use helper_obligation::{
    HelperObligationBinding, HelperObligationOutcome, HelperObligationReport,
};
pub use helper_obligation_descriptor::HelperObligationDescriptor;
pub use ir::{Concurrency, Job, Step, StepKind, Trigger, WorkflowIr};
pub use jobs::{
    CI_WORKFLOW_PATH, CRATE_JOB_ID_PREFIX, FRESHNESS_CRON_WEEKLY, FRESHNESS_WORKFLOW_PATH,
    PLAN_DISPLAY_NAME, PLAN_JOB_ID, REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, REQUIRED_JOB_ID,
    RequiredCheckMigration, STALE_WORKFLOW_PATHS, ScheduleTrigger, TOFU_DISPLAY_PREFIX,
    TOFU_JOB_ID_PREFIX, ValidatorKind, WORKFLOW_DISPLAY_NAME, WORKLOAD_DISPLAY_PREFIX,
    WORKLOAD_JOB_ID_PREFIX, assign_crate_job_ids, crate_display_label, crate_display_name,
    is_crate_job_id, is_safe_display_name, slugify_segment, tofu_display_name, validate_job_id,
    workload_display_name,
};
pub use mbx_export_descriptor::{MbxCacheDomain, MbxExportDescriptor, MbxOwnerIdentity};
pub use mbx_producer::PureMbxProducer;
pub use native_tools::{CompiledNativeExecRecipe, NativeCredentialScope};
pub use native_validation_descriptor::NativeValidationDescriptor;
pub use needs::{
    NEEDS_CHANNEL_ENV, NEEDS_CHANNEL_EXPRESSION, NEEDS_EXPECTED_ENV, NeedsConclusions,
};
pub use permissions::{PermissionLevel, Permissions};
pub use plan::{
    MatrixEntry, ObligationDecision, Plan, PlanGenerator, PlanMatrix, PlanObligation, PlanPackage,
    PlanRunner, WorkflowEvent, validate_matrix_run,
};
pub use producer_inventory::{
    ProducerAdmission, ProducerEventContext, ProducerInventory, ProducerPolicy, ProducerRole,
};
pub use qualification::{
    FinalCounts, FinalReport, FinalStatus, JobConclusion, RequiredJobResult,
    final_report_id_for_run, final_report_relpath, join_runner_temp, matrix_report_relpath,
    task_report_relpath, validate_final_report_id,
};
pub use report::{
    CacheLayer, CacheOutcome, CacheResult, MatrixReport, MatrixStatus, MatrixTaskEntry,
    NotSelectedReason, TaskReport, TaskStatus,
};
pub use scope::VerificationScope;
pub use source_helper::{
    CompiledRustReportRecipe, CompiledSourceHelper, CompilerDriver, HelperInvocation,
    RustCompilerOperation, RustCompilerTools, RustReportFrame, SOURCE_HELPER_ARGUMENT_BYTES_MAX,
    SourceBoundHelper, SourceBoundOperation, compiled_source_sha256, quote_literal_run_arg,
};
pub use source_producer::{ProducerTerminalError, SourceProducer, SourceProducerRole};
pub use step::StepId;
pub use task_execution_identity::TaskExecutionIdentity;
pub use timeout::JobTimeout;
pub use timing::{TaskTiming, TaskTimingSource};
pub use tool_producer::{PureToolProducer, ToolCacheDescriptor, ToolCacheDomain};
pub use tool_producer_selection::{
    PLAN_CARGO_FALLBACK_CONDITION, PLAN_CARGO_FALLBACK_OUTPUT, ToolProducerSelection,
};
pub use trust::{Trust, trust_for_event};

pub use outputs::{ActionOutput, JobOutput, StepOutputRef};

pub use pages::{NativePagesActions, NativePagesDeploy};

pub use native_publish::{
    NativeOciPublishEnvironment, NativePublishBinding, NativePublishPreparation, NativePublishRole,
};
