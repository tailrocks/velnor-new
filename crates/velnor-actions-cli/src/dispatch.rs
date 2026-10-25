use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    ACQUIRE_PREVIEW_SOURCE_OP, ACQUIRE_RELEASE_LEAVES_OP, ADMIT_HOMEBREW_TAP_OP,
    ADMIT_RELEASE_AGGREGATE_OP, ADMIT_STABLE_TAG_ORIGIN_OP, BUN_ASSETS_EXPORT_OP,
    BUN_ASSETS_IMPORT_OP, BUN_ASSETS_PREPARE_OP, BUN_REGISTRY_INSTALL_OP,
    CAPTURE_REVIEW_COMPLETION_OP, DYNAMIC_MATRIX_OUTPUT_MODE, EMIT_REVIEW_COMPLETION_OP,
    EXECUTE_CHECK_OP, EXECUTE_RELEASE_PRODUCER_OP, FETCH_OP, FINALIZE_PREVIEW_SOURCE_OP,
    GITHUB_EVIDENCE_FETCH_OP, GITHUB_EVIDENCE_IMPORT_OP, GRADLE_RUN_OP, MERGE_HOMEBREW_TAP_PR_OP,
    MERGE_OP, OrchestratorError, PLAN_MATRIX_OUTPUT_MODE_ENV, PLAN_OP, PLAYWRIGHT_INSTALL_OP,
    PREPARE_HOMEBREW_TAP_OP, PRESEED_MANIFEST_OP, PRIVATE_FETCH_OP, PRIVATE_REVOKE_OP,
    PUBLISH_PACKAGE_RELEASE_OP, PlanOutputMode, RECOVER_PACKAGE_RELEASE_OP, REPORT_OP,
    REQUEST_FILE_ENV, SELECT_PREVIEW_SOURCE_OP, SELECT_RELEASE_TAP_CONTINUATION_OP,
    SELECT_STABLE_RELEASE_SOURCE_OP, SIGN_PACKAGE_RELEASE_OP, TASK_EXECUTION_RESOLVER_OP,
    WRITE_HOMEBREW_TAP_PR_OP, WRITE_REQUEST_OP, WRITE_STABLE_TAG_INTENT_OP,
    WRITE_STABLE_TAG_ORIGIN_OP, execute_check, merge_internal, merge_passed, plan_internal,
    plan_outputs, publish_final_report, publish_plan_files, resolve_task_execution,
    response_path_for, retrieve_reports, run_bun_registry_install, run_github_evidence_fetch,
    run_github_evidence_import, run_gradle_run, run_playwright_install, run_private_fetch,
    run_private_revoke, write_preseed_manifest, write_request, write_task_report,
};

#[path = "dispatch/bun_assets.rs"]
mod bun_assets;
#[path = "dispatch/fileless.rs"]
mod fileless;
#[path = "dispatch_preview_production.rs"]
mod preview_production;
mod public;
#[path = "dispatch_review_completion.rs"]
mod review_completion;
#[path = "dispatch_tap_continuation.rs"]
mod tap_continuation;
pub(crate) use public::run_public;
#[path = "dispatch_owned_tool_publication.rs"]
mod owned_publication;
#[path = "dispatch_package_release.rs"]
mod package_release;
#[path = "dispatch_package_release_publish.rs"]
mod package_release_publish;
#[path = "dispatch_package_release_recovery.rs"]
mod package_release_recovery;
#[path = "dispatch_tap_merge.rs"]
mod tap_merge;
#[path = "dispatch_tap_prepare.rs"]
mod tap_prepare;

const OP_ENV: &str = "VELNOR_INTERNAL_OP";

const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";

const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

const GITHUB_EVENT_ENV: &str = "GITHUB_EVENT_NAME";

const GITHUB_EVENT_PATH_ENV: &str = "GITHUB_EVENT_PATH";

mod operation;
use operation::{InternalOp, from_env as internal_op};

#[derive(Debug)]
struct InternalRequest {
    op: InternalOp,

    path: PathBuf,
}

pub(crate) fn try_internal() -> Option<ExitCode> {
    let request = gate_request()?;
    Some(run_internal(&request))
}

fn gate_request() -> Option<InternalRequest> {
    let op = internal_op()?;
    if fileless::uses_runner_temp_gate(op) {
        return fileless::gate_runner_temp_operation(op);
    }
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
        InternalOp::WriteRequest => {
            if !request_can_be_materialized(&path) {
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
        InternalOp::PreviewSourceSelection | InternalOp::PreviewSourceFinalization => {
            return package_release::gate_preview_source_seed_request(&path)
                .map(|path| InternalRequest { op, path });
        }
        InternalOp::PreviewSourceAcquisition => {
            return package_release::gate_preview_source_acquisition(&path)
                .map(|path| InternalRequest { op, path });
        }
        InternalOp::ResolveQualification => {
            if !crate::dispatch_qualification::request_is_eligible(&path) {
                return None;
            }
        }
        InternalOp::StableSourceSelection
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
        InternalOp::TapAdmission => {
            return tap_prepare::gate_admission_request(&path)
                .map(|path| InternalRequest { op, path });
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
        InternalOp::Fetch
        | InternalOp::Report
        | InternalOp::GitHubEvidenceFetch
        | InternalOp::GitHubEvidenceImport
        | InternalOp::BunRegistryInstall
        | InternalOp::BunAssetsExport
        | InternalOp::BunAssetsPrepare
        | InternalOp::BunAssetsImport
        | InternalOp::GradleRun
        | InternalOp::PlaywrightInstall
        | InternalOp::PrivateFetch
        | InternalOp::PrivateRevoke
        | InternalOp::PreseedManifest
        | InternalOp::RepoPolicy
        | InternalOp::ExecuteCheck
        | InternalOp::ExecuteNativeHomebrewCheck
        | InternalOp::QualifyHomebrew
        | InternalOp::SeedParallelTargets
        | InternalOp::ResolveTaskExecution => {}
    }
    Some(InternalRequest { op, path })
}

fn request_can_be_materialized(path: &Path) -> bool {
    !path.exists()
        && env::var(GITHUB_EVENT_ENV).is_ok_and(|name| !name.is_empty())
        && env::var_os(GITHUB_EVENT_PATH_ENV).is_some_and(|value| !value.is_empty())
}

fn unit_exit(result: Result<(), OrchestratorError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&error.to_string()),
    }
}

fn run_internal(request: &InternalRequest) -> ExitCode {
    if let Some(code) = bun_assets::run(request.op) {
        return code;
    }
    match request.op {
        InternalOp::PreviewSourceSelection => {
            preview_production::native(velnor_actions_orchestrator::select_preview_source_from_env())
        }
        InternalOp::PreviewSourceFinalization => preview_production::native(
            velnor_actions_orchestrator::finalize_preview_source_from_env(),
        ),
        InternalOp::PreviewSourceAcquisition => preview_production::native(
            velnor_actions_orchestrator::acquire_preview_source_from_env(),
        ),
        InternalOp::ExecuteCheck => unit_exit(execute_check().map(|_| ())),
        InternalOp::SeedParallelTargets => {
            unit_exit(velnor_actions_orchestrator::seed_parallel_targets_from_env())
        }
        InternalOp::QualifyHomebrew => {
            unit_exit(velnor_actions_orchestrator::execute_homebrew_qualification_from_env())
        }
        InternalOp::ExecuteNativeHomebrewCheck => unit_exit(
            velnor_actions_orchestrator::execute_native_homebrew_check_from_env().map(|_| ()),
        ),
        InternalOp::StableSourceSelection => preview_production::native(
            velnor_actions_orchestrator::select_stable_release_source_from_env(),
        ),
        InternalOp::StableTagIntent => preview_production::native(
            velnor_actions_orchestrator::write_stable_tag_intent_from_env(),
        ),
        InternalOp::StableTagOrigin => preview_production::native(
            velnor_actions_orchestrator::write_stable_tag_origin_from_env(),
        ),
        InternalOp::StableTagAdmission => preview_production::native(
            velnor_actions_orchestrator::admit_stable_tag_origin_from_env(),
        ),
        InternalOp::ExecuteReleaseProducer => preview_production::produce(),
        InternalOp::AcquireReleaseLeaves => preview_production::acquire(),
        InternalOp::TapContinuation => tap_continuation::run(),
        InternalOp::CaptureReviewCompletion => review_completion::capture(),
        InternalOp::EmitReviewCompletion => review_completion::emit(),
        InternalOp::AdmitReleaseAggregate => preview_production::admit(),
        InternalOp::WriteRequest => unit_exit(write_request().map(|_| ())),
        InternalOp::Plan => run_plan_internal(&request.path),
        InternalOp::Merge => run_merge_internal(&request.path),
        InternalOp::Fetch => unit_exit(retrieve_reports().map(|_| ())),
        InternalOp::Report => unit_exit(write_task_report().map(|_| ())),
        InternalOp::GradleRun => unit_exit(run_gradle_run()),
        InternalOp::BunRegistryInstall => unit_exit(run_bun_registry_install()),
        InternalOp::PlaywrightInstall => unit_exit(run_playwright_install()),
        InternalOp::PrivateFetch => unit_exit(run_private_fetch()),
        InternalOp::PrivateRevoke => unit_exit(run_private_revoke()),
        InternalOp::PreseedManifest => unit_exit(write_preseed_manifest()),
        InternalOp::BunAssetsExport
        | InternalOp::BunAssetsPrepare
        | InternalOp::BunAssetsImport => ExitCode::FAILURE,
        InternalOp::RepoPolicy => crate::dispatch_repo_policy::run(&request.path),
        InternalOp::ResolveQualification => crate::dispatch_qualification::run(&request.path),
        InternalOp::ResolveTaskExecution => match resolve_task_execution() {
            Ok(frame) => match std::io::stdout().lock().write_all(&frame) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => fail_internal(&format!("write task execution frame: {error}")),
            },
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::GitHubEvidenceFetch => unit_exit(run_github_evidence_fetch()),
        InternalOp::GitHubEvidenceImport => unit_exit(run_github_evidence_import()),
        InternalOp::PackageSigner => package_release::run(),
        InternalOp::PackagePublisher => package_release_publish::run(),
        InternalOp::PackageRecovery => package_release_recovery::run(),
        InternalOp::TapPrepare => tap_prepare::run(),
        InternalOp::TapAdmission => tap_prepare::run_admission(),
        InternalOp::TapWriter => tap_prepare::run_writer(),
        InternalOp::TapMerge => tap_merge::run(),
    }
}

fn run_plan_internal(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return fail_internal(&format!("read request: {error}")),
    };
    let mode = match env::var_os(PLAN_MATRIX_OUTPUT_MODE_ENV) {
        None => PlanOutputMode::Static,
        Some(value) if value == DYNAMIC_MATRIX_OUTPUT_MODE => PlanOutputMode::DynamicMatrix,
        Some(_) => return fail_internal("bad_plan_matrix_output_mode"),
    };
    let response = match plan_internal(&text) {
        Ok(response) => response,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let sibling = match response_path_for(path) {
        Ok(sibling) => sibling,
        Err(error) => return fail_internal(&error.to_string()),
    };
    if let Err(error) = fs::write(&sibling, &response) {
        return fail_internal(&format!("write response: {error}"));
    }
    let outputs = match plan_outputs(&response, mode) {
        Ok(outputs) => outputs,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing runner temp");
    };
    let Some(output_path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing github output");
    };
    if let Err(error) = publish_plan_files(&response, &Path::new(&runner_temp).join("velnor")) {
        return fail_internal(&error.to_string());
    }
    let mut body = String::new();
    for (name, value) in outputs.step_outputs() {
        body.push_str(name);
        body.push('=');
        body.push_str(value);
        body.push('\n');
    }
    match fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&output_path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(body.as_bytes())
        }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("append outputs: {error}")),
    }
}

fn run_merge_internal(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return fail_internal(&format!("read request: {error}")),
    };
    let response = match merge_internal(&text) {
        Ok(response) => response,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let sibling = match response_path_for(path) {
        Ok(sibling) => sibling,
        Err(error) => return fail_internal(&error.to_string()),
    };
    if let Err(error) = fs::write(&sibling, &response) {
        return fail_internal(&format!("write response: {error}"));
    }
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing runner temp");
    };
    if let Err(error) = publish_final_report(&response, &Path::new(&runner_temp).join("velnor")) {
        return fail_internal(&error.to_string());
    }
    match merge_passed(&response) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => fail_internal(&error.to_string()),
    }
}

#[path = "dispatch/errors.rs"]
mod errors;
pub(crate) use errors::{fail_internal, fail_public, working_dir};
