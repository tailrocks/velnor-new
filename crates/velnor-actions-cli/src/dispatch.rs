//! Typed dispatch for public commands and the private file entrypoint.
//!
//! The private gate requires `VELNOR_INTERNAL_OP` plus the exact request file
//! in `VELNOR_REQUEST_FILE`: `write-request-v1` needs GitHub event env and no
//! pre-existing file, `plan-v1`/`merge-v1` need a
//! pre-existing request file, `fetch-reports-v1`/`write-task-report-v1`
//! need runner temp plus the numeric run ID instead, and
//! `write-preseed-manifest-v1` needs runner temp only, and
//! `resolve-qualification-v1` requires a dispatch event, request file, and
//! read-only GitHub token. `resolve-task-execution-v1` requires runner temp
//! and the run ID; it emits only the plan-bound task data frame. Anything else
//! falls through to Clap, so public behavior is byte-identical with or without
//! the environment set.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    ACQUIRE_RELEASE_LEAVES_OP, ADMIT_HOMEBREW_TAP_OP, ADMIT_RELEASE_AGGREGATE_OP,
    ADMIT_RELEASE_POLICY_OP, ADMIT_STABLE_TAG_ORIGIN_OP, BUN_ASSETS_EXPORT_OP,
    BUN_ASSETS_IMPORT_OP, BUN_ASSETS_PREPARE_OP, BUN_REGISTRY_INSTALL_OP, CACHE_RECEIPT_OP,
    CAPTURE_REVIEW_COMPLETION_OP, EMIT_REVIEW_COMPLETION_OP, EXECUTE_CHECK_OP,
    EXECUTE_RELEASE_PRODUCER_OP, FETCH_OP, GRADLE_RUN_OP, MERGE_HOMEBREW_TAP_PR_OP, MERGE_OP,
    OrchestratorError, PLAN_OP, PLAYWRIGHT_INSTALL_OP, PREPARE_HOMEBREW_TAP_OP,
    PRESEED_MANIFEST_OP, PRIVATE_FETCH_OP, PRIVATE_REVOKE_OP, PUBLISH_PACKAGE_RELEASE_OP,
    RECOVER_PACKAGE_RELEASE_OP, REPORT_OP, REQUEST_FILE_ENV, SELECT_RELEASE_TAP_CONTINUATION_OP,
    SELECT_STABLE_RELEASE_SOURCE_OP, SIGN_PACKAGE_RELEASE_OP, SOURCE_EVIDENCE_OP,
    TASK_EXECUTION_RESOLVER_OP, WRITE_HOMEBREW_TAP_PR_OP, WRITE_REQUEST_OP,
    WRITE_STABLE_TAG_INTENT_OP, WRITE_STABLE_TAG_ORIGIN_OP,
};

#[path = "dispatch/diagnostics.rs"]
mod diagnostics;
pub(crate) use diagnostics::{fail_internal, fail_public, working_dir};
#[path = "dispatch/bun_assets.rs"]
mod bun_assets;
#[path = "dispatch/fileless.rs"]
mod fileless;
#[path = "dispatch_preview_production.rs"]
mod preview_production;
#[path = "dispatch_review_completion.rs"]
mod review_completion;
#[path = "dispatch_tap_continuation.rs"]
mod tap_continuation;

mod public;
pub(crate) use public::run_public;
#[path = "dispatch_cache_bind.rs"]
mod cache_bind;
#[path = "dispatch_cache_owner_evidence.rs"]
mod cache_owner_evidence;
#[path = "dispatch_cache_receipt.rs"]
mod cache_receipt;
#[path = "dispatch_cache_runtime.rs"]
mod cache_runtime;
#[path = "dispatch_cache_tar.rs"]
mod cache_tar;
#[path = "dispatch_cache_tar_launcher.rs"]
mod cache_tar_launcher;
#[path = "dispatch_internal_run.rs"]
mod internal_run;
#[path = "dispatch_cache_normal_tofu_preflight.rs"]
mod normal_tofu_cache_preflight;
#[path = "dispatch_owned_tool_publication.rs"]
mod owned_publication;
#[path = "dispatch_package_release.rs"]
mod package_release;
#[path = "dispatch_package_release_publish.rs"]
mod package_release_publish;
#[path = "dispatch_package_release_recovery.rs"]
mod package_release_recovery;
#[path = "dispatch_run_context.rs"]
mod run_context;
#[path = "dispatch_source_evidence_gate.rs"]
mod source_evidence_gate;
#[path = "dispatch_tap_merge.rs"]
mod tap_merge;
#[path = "dispatch_tap_prepare.rs"]
mod tap_prepare;

/// Environment variable selecting the private operation. Never printed.
const OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Environment variable carrying the `$GITHUB_OUTPUT` path. Never printed.
const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";
/// Environment variable carrying the runner temp dir. Never printed.
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
/// Environment variable carrying the triggering event name. Never printed.
const GITHUB_EVENT_ENV: &str = "GITHUB_EVENT_NAME";
/// Environment variable carrying the event payload path. Never printed.
const GITHUB_EVENT_PATH_ENV: &str = "GITHUB_EVENT_PATH";

fn unit_exit(result: Result<(), OrchestratorError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&error.to_string()),
    }
}

#[path = "dispatch/operation.rs"]
mod operation;
use operation::from_env as internal_op;
use operation::{InternalOp, InternalRequest};

/// Run the private file entrypoint when the gate is satisfied.
///
/// Returns `None` when the gate is not satisfied so the caller falls through
/// to Clap; bare invocations then fail with the usage diagnostic (exit 2),
/// keeping public behavior byte-identical with or without the environment.
pub(crate) fn try_internal() -> Option<ExitCode> {
    let request = gate_request()?;
    Some(internal_run::run_internal(&request))
}

/// Route `tar` and `gtar` invocations to the cache adapter.
///
/// One `--version` argument does not need runner context. Archive operations
/// need the exact operation tag and runner context before the adapter reads
/// stage inputs.
pub(crate) fn try_cache_tar_entrypoint() -> Option<ExitCode> {
    let mut args = env::args_os();
    let program = args.next()?;
    if !matches!(
        Path::new(&program).file_name(),
        Some(name) if name == "tar" || name == "gtar"
    ) {
        return None;
    }
    let args: Vec<_> = args.collect();
    if args.len() == 1 && args[0] == "--version" {
        return Some(cache_tar::run(&args));
    }
    if !env::var(OP_ENV).is_ok_and(|operation| operation == cache_tar::OP)
        || !cache_runner_context_is_valid()
    {
        return Some(fail_internal("cache_tar_runner_context_invalid"));
    }
    Some(cache_tar::run(&args))
}

/// Check the private gate: known op plus request-file presence by op.
///
/// Fetch and report take no request file: they need the runner-temp
/// velnor directory plus the numeric run ID instead. The preseed-manifest op
/// takes no request file either: runner temp scopes its output.
fn gate_request() -> Option<InternalRequest> {
    let op = internal_op()?;
    if matches!(
        op,
        InternalOp::BindQualificationCache
            | InternalOp::ObserveQualificationCacheRuntime
            | InternalOp::ObserveQualificationCacheRoots
            | InternalOp::RecordQualificationCacheOwnerLayer
            | InternalOp::AggregateQualificationCacheOwner
            | InternalOp::ImportQualificationCachePayload
            | InternalOp::ExportQualificationCachePayload
            | InternalOp::NormalToolsCacheStagePath
            | InternalOp::ImportNormalToolsCachePayload
            | InternalOp::ExportNormalToolsCachePayload
            | InternalOp::InstallCacheTarLaunchers
            | InternalOp::PreflightNormalTofuCacheRestore
            | InternalOp::WriteQualificationCacheReceipt
    ) {
        return cache_runner_request(op);
    }
    if op == InternalOp::WriteQualificationSourceEvidence {
        return source_evidence_gate::request(op);
    }
    if fileless::uses_runner_temp_gate(op) {
        return fileless::gate_runner_temp_operation(op);
    }
    gate_request_file(op)
}

fn gate_request_file(op: InternalOp) -> Option<InternalRequest> {
    let path = env::var_os(REQUEST_FILE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    gate_file_request(op, path)
}

fn gate_file_request(op: InternalOp, path: PathBuf) -> Option<InternalRequest> {
    match op {
        InternalOp::TapContinuation => {
            return tap_continuation::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::CaptureReviewCompletion | InternalOp::EmitReviewCompletion => {
            return review_completion::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::TapAdmission => {
            return tap_prepare::gate_admission_request(&path)
                .map(|path| InternalRequest { op, path });
        }
        InternalOp::WriteRequest => {
            if path.exists() {
                return None;
            }
            if !env::var(GITHUB_EVENT_ENV).is_ok_and(|name| !name.is_empty()) {
                return None;
            }
            if env::var_os(GITHUB_EVENT_PATH_ENV).is_none_or(|value| value.is_empty()) {
                return None;
            }
        }
        InternalOp::Plan | InternalOp::Merge => {
            if !path.is_file() {
                return None;
            }
        }
        InternalOp::ExecuteReleaseProducer => {
            return package_release::gate_producer_request(&path)
                .map(|path| InternalRequest { op, path });
        }
        InternalOp::ResolveQualification => {
            if !crate::dispatch_qualification::request_is_eligible(&path) {
                return None;
            }
        }
        InternalOp::StableSourceSelection
        | InternalOp::ReleasePolicyAdmission
        | InternalOp::StableTagIntent
        | InternalOp::StableTagOrigin
        | InternalOp::StableTagAdmission
        | InternalOp::PackageSigner
        | InternalOp::PackagePublisher
        | InternalOp::PackageRecovery
        | InternalOp::AdmitReleaseAggregate
        | InternalOp::AcquireReleaseLeaves => {
            return package_release::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::TapPrepare => {
            return tap_prepare::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::TapWriter => {
            return tap_prepare::gate_writer_request(&path)
                .map(|path| InternalRequest { op, path });
        }
        InternalOp::TapMerge => {
            return tap_merge::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        op if request_file_only_gate(op) => {}
        _ => return None,
    }
    Some(InternalRequest { op, path })
}

fn request_file_only_gate(op: InternalOp) -> bool {
    matches!(
        op,
        InternalOp::Fetch
            | InternalOp::Report
            | InternalOp::BunRegistryInstall
            | InternalOp::PrivateFetch
            | InternalOp::PrivateRevoke
            | InternalOp::PreseedManifest
            | InternalOp::RepoPolicy
            | InternalOp::ExecuteCheck
            | InternalOp::BindQualificationCache
            | InternalOp::ObserveQualificationCacheRuntime
            | InternalOp::ObserveQualificationCacheRoots
            | InternalOp::RecordQualificationCacheOwnerLayer
            | InternalOp::AggregateQualificationCacheOwner
            | InternalOp::ImportQualificationCachePayload
            | InternalOp::ExportQualificationCachePayload
            | InternalOp::NormalToolsCacheStagePath
            | InternalOp::ImportNormalToolsCachePayload
            | InternalOp::ExportNormalToolsCachePayload
            | InternalOp::InstallCacheTarLaunchers
            | InternalOp::PreflightNormalTofuCacheRestore
            | InternalOp::WriteQualificationCacheReceipt
            | InternalOp::WriteQualificationSourceEvidence
            | InternalOp::ResolveTaskExecution
    )
}

/// Require both GitHub run coordinates before entering cache operations.
fn cache_runner_request(op: InternalOp) -> Option<InternalRequest> {
    if !run_context::valid_run_coordinates() {
        return None;
    }
    runner_velnor_dir().map(|path| InternalRequest { op, path })
}

/// Check the workflow context required by the cache tar adapter.
pub(crate) fn cache_runner_context_is_valid() -> bool {
    env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true")
        && run_context::valid_run_coordinates()
        && env::var_os(RUNNER_TEMP_ENV).is_some_and(|value| !value.is_empty())
}

/// Runner-temp velnor directory for file-less private operations.
///
/// `None` when `RUNNER_TEMP` is unset or empty; shared by the fetch,
/// report, and preseed-manifest gate branches so the scoping rule has
/// one definition.
fn runner_velnor_dir() -> Option<PathBuf> {
    env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(|temp| Path::new(&temp).join("velnor"))
}
