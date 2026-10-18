//! Private gate for the credential-free native tap updater.

use std::io::Write;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use velnor_actions_orchestrator::{
    OrchestratorError, TapPromotionOutcome, TapPromotionReceipt, prepare_homebrew_tap_from_env,
    write_homebrew_tap_pr_from_env,
};

const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const REQUEST_ENV: &str = "VELNOR_REQUEST_FILE";
const PLAN_FILE: &str = "package-release-plan.json";
const PACKAGE_INPUT_DIR: &str = "preview-publish-input";
const CANDIDATE_DIR: &str = "tap-candidate";

/// Admit only the fixed package plan plus retained signed release assets.
pub(super) fn gate_request(path: &Path) -> Option<PathBuf> {
    gate_common(path, false)
}

/// Admit only the fixed request, signed package handoff, candidate files, and App token.
pub(super) fn gate_writer_request(path: &Path) -> Option<PathBuf> {
    gate_common(path, true)
}

fn gate_common(path: &Path, writer: bool) -> Option<PathBuf> {
    if !env::var("GITHUB_RUN_ID").is_ok_and(|id| !id.is_empty()) {
        return None;
    }
    let runner_temp = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty())?;
    let runner_temp = PathBuf::from(runner_temp);
    if !runner_temp.is_absolute() {
        return None;
    }
    let root = runner_temp.join("velnor");
    if !is_real_directory(&root)
        || path != root.join(PLAN_FILE)
        || env::var_os(REQUEST_ENV).as_deref() != Some(path.as_os_str())
        || !package_input_available(&root)
    {
        return None;
    }
    if writer {
        for key in [
            "VELNOR_TAP_TOKEN",
            "VELNOR_TAP_APP_ID",
            "VELNOR_TAP_APP_SLUG",
            "VELNOR_TAP_INSTALLATION_ID",
            "VELNOR_TAP_POLICY",
        ] {
            if !env::var(key).is_ok_and(|value| !value.is_empty()) {
                return None;
            }
        }
        let candidate = root.join(CANDIDATE_DIR);
        if !is_real_directory(&candidate)
            || !is_regular_file(&candidate.join("formula.rb"))
            || !is_regular_file(&candidate.join("candidate.json"))
            || !env::var_os("GITHUB_OUTPUT")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .is_some_and(|path| path.is_absolute() && is_regular_file(&path))
        {
            return None;
        }
    }
    let request = fs::symlink_metadata(path).ok()?;
    if request.file_type().is_symlink()
        || !request.is_file()
        || request.len() == 0
        || request.len() > 1024 * 1024
    {
        return None;
    }
    Some(path.to_path_buf())
}

pub(crate) fn package_input_available(root: &Path) -> bool {
    match env::var("GITHUB_EVENT_NAME").as_deref() {
        Ok("push") => is_real_directory(&root.join(PACKAGE_INPUT_DIR)),
        Ok("workflow_dispatch") => {
            env::var("VELNOR_RELEASE_ORIGINAL_RUN_ID")
                .is_ok_and(|value| value.parse::<u64>().is_ok_and(|id| id > 0))
                && env::var("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")
                    .is_ok_and(|value| value.parse::<u32>().is_ok_and(|attempt| attempt > 0))
        }
        _ => false,
    }
}

/// Run the unprivileged updater and literal-only validator.
pub(super) fn run() -> ExitCode {
    match prepare_homebrew_tap_from_env() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => fail_error(&error),
    }
}

/// Reauthenticate, create/reconcile one PR, and emit the exact verified PR receipt.
pub(super) fn run_writer() -> ExitCode {
    match write_homebrew_tap_pr_from_env() {
        Ok(receipt) => append_writer_outputs(&receipt),
        Err(error) => fail_error(&error),
    }
}

fn append_writer_outputs(receipt: &TapPromotionReceipt) -> ExitCode {
    let Some(path) = env::var_os("GITHUB_OUTPUT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return super::fail_internal("tap_writer_output_missing");
    };
    if !path.is_absolute() {
        return super::fail_internal("tap_writer_output_invalid");
    }
    if !is_regular_file(&path) {
        return super::fail_internal("tap_writer_output_invalid");
    }
    let outputs = writer_outputs(receipt);
    match fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(outputs.as_bytes()))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&format!("tap_writer_output_write:{:?}", error.kind())),
    }
}

fn writer_outputs(receipt: &TapPromotionReceipt) -> String {
    match &receipt.outcome {
        TapPromotionOutcome::PullRequest {
            pull_request,
            head_sha,
            base_sha,
            candidate_proof_digest,
            ..
        } => format!(
            "promotion_state=pull_request\npr_number={pull_request}\nhead_sha={head_sha}\nbase_sha={base_sha}\ncandidate_proof_digest={candidate_proof_digest}\n"
        ),
        TapPromotionOutcome::AlreadyPromoted {
            pull_request,
            head_sha,
            base_sha,
            candidate_proof_digest,
            merge_sha,
            merged_at,
            ..
        } => format!(
            "promotion_state=already_promoted\npr_number={pull_request}\nhead_sha={head_sha}\nbase_sha={base_sha}\ncandidate_proof_digest={candidate_proof_digest}\nmerge_sha={merge_sha}\nmerged_at={merged_at}\n"
        ),
    }
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn fail_error(error: &OrchestratorError) -> ExitCode {
    let message = error.to_string().replace(['\n', '\r'], " ");
    eprintln!("velnor-actions: internal request failed: {message}");
    ExitCode::from(1)
}
