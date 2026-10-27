// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use velnor_actions_orchestrator::{
    OrchestratorError, TapPromotionOutcome, TapPromotionReceipt, admit_homebrew_tap_from_env,
    prepare_homebrew_tap_from_env, write_homebrew_tap_pr_from_env,
};

const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const REQUEST_ENV: &str = "VELNOR_REQUEST_FILE";
const PLAN_FILE: &str = "package-release-policy.json";
const PACKAGE_INPUT_DIR: &str = "release-publish-input";
const CANDIDATE_DIR: &str = "tap-candidate";

pub(super) fn gate_request(path: &Path) -> Option<PathBuf> {
    if ["GH_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .any(|key| env::var_os(key).is_some())
        || !matches!(
            env::var("GITHUB_JOB").as_deref(),
            Ok("preview-tap-prepare" | "stable-tap-prepare")
        )
        || !env::var("VELNOR_TAP_ADMISSION_DIGEST").is_ok_and(|value| valid_digest(&value))
    {
        return None;
    }
    let request = gate_common(path, false)?;
    let record = path.parent()?.join("tap-admission.json");
    is_regular_file(&record).then_some(request)
}

pub(super) fn gate_admission_request(path: &Path) -> Option<PathBuf> {
    if !matches!(
        env::var("GITHUB_JOB").as_deref(),
        Ok("preview-tap-prepare" | "stable-tap-prepare")
    ) || !env::var("GH_TOKEN").is_ok_and(|value| !value.is_empty())
        || !env::var_os("GITHUB_OUTPUT")
            .map(PathBuf::from)
            .is_some_and(|path| path.is_absolute() && is_regular_file(&path))
    {
        return None;
    }
    gate_common(path, false)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn run_admission() -> ExitCode {
    let digest = match admit_homebrew_tap_from_env() {
        Ok(digest) if valid_digest(&digest) => digest,
        Ok(_) => return super::fail_internal("tap_admission_output_digest"),
        Err(error) => return fail_error(&error),
    };
    let Some(path) = env::var_os("GITHUB_OUTPUT")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && is_regular_file(path))
    else {
        return super::fail_internal("tap_admission_output_missing");
    };
    match fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "admission_digest={digest}"))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => super::fail_internal("tap_admission_output_write"),
    }
}

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
        Ok("workflow_run") => is_real_directory(&root.join(PACKAGE_INPUT_DIR)),
        Ok("workflow_dispatch") => {
            original_run_available()
                || (stable_production_context() && is_real_directory(&root.join(PACKAGE_INPUT_DIR)))
        }
        Ok("schedule") => continuation_context() && original_run_available(),
        _ => false,
    }
}

fn stable_production_context() -> bool {
    let Ok(repository) = env::var("GITHUB_REPOSITORY") else {
        return false;
    };
    !repository.is_empty()
        && env::var("GITHUB_REF").as_deref() == Ok("refs/heads/main")
        && env::var("GITHUB_WORKFLOW_REF").is_ok_and(|value| {
            value == format!("{repository}/.github/workflows/binary-release.yml@refs/heads/main")
        })
        && env::var("VELNOR_STABLE_OPERATION").as_deref() == Ok("release")
}

fn original_run_available() -> bool {
    env::var("VELNOR_RELEASE_ORIGINAL_RUN_ID")
        .is_ok_and(|value| value.parse::<u64>().is_ok_and(|id| id > 0))
        && env::var("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")
            .is_ok_and(|value| value.parse::<u32>().is_ok_and(|attempt| attempt > 0))
}

fn continuation_context() -> bool {
    let Ok(repository) = env::var("GITHUB_REPOSITORY") else {
        return false;
    };
    !repository.is_empty()
        && env::var("GITHUB_REF").as_deref() == Ok("refs/heads/main")
        && env::var("GITHUB_WORKFLOW_REF").is_ok_and(|value| {
            ["preview", "stable"].iter().any(|channel| {
                value
                    == format!(
                        "{repository}/.github/workflows/{channel}-tap-reconcile.yml@refs/heads/main"
                    )
            })
        })
}

pub(super) fn run() -> ExitCode {
    match prepare_homebrew_tap_from_env() {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => fail_error(&error),
    }
}

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
        TapPromotionOutcome::InitializedByCutover {
            cutover_pull_request,
            head_sha,
            base_sha,
            merge_sha,
            merged_at,
            marker_sha256,
        } => format!(
            "promotion_state=initialized_by_cutover\ncutover_pr_number={cutover_pull_request}\nhead_sha={head_sha}\nbase_sha={base_sha}\nmerge_sha={merge_sha}\nmerged_at={merged_at}\nmarker_sha256={marker_sha256}\n"
        ),
        TapPromotionOutcome::SkippedStaleSource { source_sha } => {
            format!("promotion_state=skipped_stale_source\nsource_sha={source_sha}\n")
        }
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
