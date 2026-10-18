//! Execution handlers for validated private CLI operations.

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    DYNAMIC_MATRIX_OUTPUT_MODE, PLAN_MATRIX_OUTPUT_MODE_ENV, PlanOutputMode, execute_check,
    execute_preview_producer, merge_internal, merge_passed, plan_internal,
    plan_outputs_from_staged_admission, publish_final_report, publish_plan_files_with_outputs,
    resolve_task_execution, response_path_for, retrieve_reports, write_preseed_manifest,
    write_qualification_source_evidence, write_request, write_task_report,
};

use super::{
    GITHUB_OUTPUT_ENV, InternalOp, InternalRequest, RUNNER_TEMP_ENV, cache_bind, cache_observe,
    cache_tar_launcher, fail_internal, package_release, package_release_publish,
    package_release_recovery, run_publish_internal, tap_merge, tap_prepare,
};

/// Run one validated private operation.
pub(super) fn run_internal(request: &InternalRequest) -> ExitCode {
    match request.op {
        InternalOp::ExecuteCheck => match execute_check() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::ExecutePreviewProducer => match execute_preview_producer() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::WriteRequest => match write_request() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::Plan => run_plan_internal(&request.path),
        InternalOp::Merge => run_merge_internal(&request.path),
        InternalOp::Fetch => match retrieve_reports() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::Report => match write_task_report() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::PreseedManifest => match write_preseed_manifest() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::Publish => run_publish_internal(&request.path),
        InternalOp::RepoPolicy => crate::dispatch_repo_policy::run(&request.path),
        InternalOp::ResolveQualification => crate::dispatch_qualification::run(&request.path),
        InternalOp::BindQualificationCache => cache_bind::run(),
        InternalOp::ObserveQualificationCacheBackend => cache_observe::run(),
        InternalOp::ImportQualificationCachePayload => cache_bind::transfer::run_import(),
        InternalOp::ExportQualificationCachePayload => cache_bind::transfer::run_export(),
        InternalOp::NormalToolsCacheStagePath => cache_bind::normal_stage::run(),
        InternalOp::ImportNormalToolsCachePayload => cache_bind::transfer::normal::run_import(),
        InternalOp::ExportNormalToolsCachePayload => cache_bind::transfer::normal::run_export(),
        InternalOp::InstallCacheTarLaunchers => cache_tar_launcher::run(),
        InternalOp::PreflightNormalTofuCacheRestore => super::normal_tofu_cache_preflight::run(),
        InternalOp::WriteQualificationSourceEvidence => match write_qualification_source_evidence()
        {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::ResolveTaskExecution => match resolve_task_execution() {
            Ok(frame) => match std::io::stdout().lock().write_all(&frame) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => fail_internal(&format!("write task execution frame: {error}")),
            },
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::PackageSigner => package_release::run(),
        InternalOp::PackagePublisher => package_release_publish::run(),
        InternalOp::PackageRecovery => package_release_recovery::run(),
        InternalOp::TapPrepare => tap_prepare::run(),
        InternalOp::TapWriter => tap_prepare::run_writer(),
        InternalOp::TapMerge => tap_merge::run(),
    }
}

/// Read the request, run the planner, publish files, and write outputs.
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
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing runner temp");
    };
    let outputs = match plan_outputs_from_staged_admission(&response, mode, Path::new(&runner_temp))
    {
        Ok(outputs) => outputs,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let Some(output_path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing github output");
    };
    if let Err(error) = publish_plan_files_with_outputs(
        &response,
        &Path::new(&runner_temp).join("velnor"),
        &outputs,
    ) {
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
        .and_then(|mut file| file.write_all(body.as_bytes()))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("append outputs: {error}")),
    }
}

/// Read the request, run the merge, publish the verdict, and exit with it.
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
