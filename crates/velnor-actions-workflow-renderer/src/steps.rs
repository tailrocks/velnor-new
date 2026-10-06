//! Fixed step templates over validated command strings.
//!
//! Templates fix names, env keys, and payload shapes; argv arrives validated.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, commands, marker};

pub(crate) use crate::cache_steps::tools_cache_step;
pub use crate::cache_steps::{
    CACHE_RESTORE_NAME, CACHE_SAVE_NAME, CompileDriver, MBX_ACTION_NAME, MBX_CACHE_MODE_ENV,
    MBX_PREFLIGHT_NAME, MBX_RESTORE_NAME, MBX_VERSION_CHECK_NAME, NEVER_ARCHIVE_MARKERS,
    TASK_ARTIFACTS_DIR, TOOLS_CACHE_PATH, TOOLS_CACHE_PATHS, TOOLS_RESTORE_ACTION_USES,
    TOOLS_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_NAME, TOOLS_SAVE_USES, cache_action_step,
    check_cache_step_order, check_mbx_gating, is_never_archive_path, mbx_steps_for_driver,
};
pub(crate) use crate::steps_shell::composite_shell_step;
pub use crate::steps_shell::{ambient_shell_step, shell_step};

pub use crate::action_ref::validate_uses;
pub use crate::steps_artifact::{
    ARTIFACT_NAME_OUTPUT, BASELINE_PUBLISH_UPLOAD_NAME, BASELINE_RETENTION_DAYS, PUBLISH_STEP_ID,
    baseline_publish_upload_step, crate_job_report_upload_step, download_artifact_step,
    matrix_report_upload_step, upload_artifact_step,
};
pub(crate) use crate::steps_internal::split_internal_operation;
pub use crate::steps_internal::{
    internal_step, merge_step, plan_step, publish_step, write_request_step,
};

/// Env key selecting the staged-binary internal operation.
pub const INTERNAL_OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Env key carrying the JSON request-file path.
pub const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
/// Planner operation name.
pub const PLAN_OPERATION: &str = "plan-v1";
/// Pre-seed manifest-writing operation name.
pub const WRITE_PRESEED_MANIFEST_OPERATION: &str = "write-preseed-manifest-v1";
/// Report-merge operation name.
pub const MERGE_OPERATION: &str = "merge-v1";
/// Baseline-publish operation: `publish-baseline-v1`.
pub const PUBLISH_OPERATION: &str = "publish-baseline-v1";
/// Matrix-report fetch operation name.
pub const FETCH_OPERATION: &str = "fetch-reports-v1";
/// Write-request operation name.
pub const WRITE_REQUEST_OPERATION: &str = "write-request-v1";
/// Resolve exact predecessor receipts for a typed qualification plan.
pub const RESOLVE_QUALIFICATION_OPERATION: &str = "resolve-qualification-v1";
/// Required prefix of the digest-verified staged binary path.
pub const STAGED_BINARY_PREFIX: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-";
/// Required prefix of internal request directories (expression form: shell
/// `$VAR` never expands in the `env:` position that carries this path).
pub const REQUEST_DIR_PREFIX: &str = "${{ runner.temp }}/velnor/";
/// Env key carrying the downloaded asset SHA-256.
pub const ASSET_SHA_ENV: &str = "VELNOR_ASSET_SHA256";
/// Env key carrying the downloaded asset URL.
pub const ASSET_URL_ENV: &str = "VELNOR_ASSET_URL";
/// Env key recording the release-manifest source commit (F3).
///
/// Required on every Acquire step: the manifest path records the
/// manifest commit and the lock-backed Velnor path records the lock
/// commit with identical strictness. Reviewers verify it against the
/// release.
pub const RELEASE_COMMIT_ENV: &str = "VELNOR_RELEASE_COMMIT";
/// Substrings that must never appear in rendered YAML.
pub const FORBIDDEN_TOKENS: &[&str] = &["__internal", "velnor-actions __", "velnor-actions run"];
/// Pinned `actions/upload-artifact` ref (v7.0.1, qualified 2026-09-28).
pub const UPLOAD_ARTIFACT_USES: &str =
    "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a";
/// Retention for every rendered `upload-artifact` step, in days.
///
/// Run-scoped evidence reproducible by rerun; bounded well under the
/// 90-day platform default to cap stored bytes. Every constructor
/// below emits this value as `retention-days`; no site retypes it.
pub const ARTIFACT_RETENTION_DAYS: u32 = 30;
/// Pinned `actions/download-artifact` ref (v8.0.1, qualified 2026-09-28).
pub const DOWNLOAD_ARTIFACT_USES: &str =
    "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c";
/// Candidate manifest filename inside the uploaded artifact.
pub const CANDIDATE_MANIFEST_FILE: &str = "candidate-manifest.json";
/// Directory holding the built candidate binary plus its manifest.
///
/// Shell `run:` spelling only; action inputs use
/// [`crate::CANDIDATE_OUTPUT_DIR_EXPR`].
pub const CANDIDATE_OUTPUT_DIR: &str = "$RUNNER_TEMP/velnor/candidate-output";
/// Directory receiving the downloaded candidate for qualification.
///
/// Shell `run:` spelling only; action inputs and `env:` use
/// [`crate::CANDIDATE_STAGE_DIR_EXPR`].
pub const CANDIDATE_STAGE_DIR: &str = "$RUNNER_TEMP/velnor/candidate";
/// Contract-fixed display name of the helper-staging step.
pub const ACQUIRE_NAME: &str = "Acquire Velnor";
/// Run-key expression (`r<run-id>-a<run-attempt>`, workflow-contract §3).
pub const RUN_KEY_EXPR: &str = "r${{ github.run_id }}-a${{ github.run_attempt }}";
/// Display name of the matrix-report upload step.
pub const MATRIX_REPORT_UPLOAD_NAME: &str = "Upload matrix report";
/// Display name of the per-job crate-report upload step.
pub const CRATE_REPORT_UPLOAD_NAME: &str = "Upload crate reports";
/// Contract-fixed display name of the policy cargo-deny step.
pub const DENY_STEP_NAME: &str = "Run cargo-deny";
/// Contract-fixed display name of the policy cargo-machete step.
pub const MACHETE_STEP_NAME: &str = "Run cargo-machete";

/// Reject text containing a private-subcommand or parallel token.
/// # Errors
pub fn scan_for_private_subcommands(text: &str) -> Result<(), RenderError> {
    for token in FORBIDDEN_TOKENS {
        if text.contains(token) {
            return Err(RenderError::PrivateSubcommand((*token).to_owned()));
        }
    }
    Ok(())
}

/// Checkout step without persisted credentials.
/// # Errors
pub fn checkout_step(uses: &str) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    if !uses.starts_with("actions/checkout@") {
        return Err(RenderError::BadActionRef(format!("not_checkout:{uses}")));
    }
    let with = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    let mut step = action_step("Checkout", uses, with)?;
    step.role = Some(StepRole::Checkout);
    Ok(step)
}

/// Validated pinned-action step.
///
/// Names share the step-name gate (no expressions); `with:` keys never
/// carry expressions and values only allowlisted runner spans.
/// # Errors
pub fn action_step(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    action_step_with_env(name, uses, with, BTreeMap::new())
}

/// Validated action step with step-level environment.
///
/// Same gates as [`action_step`]; `env` renders as the step's `env:`
/// map and applies to the action's main and post phases alike, which
/// is what lets a cache mode gate the post-step save while the
/// restore still runs on every event.
/// # Errors
pub fn action_step_with_env(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_name".to_owned()));
    }
    crate::expressions::check_name_content(name)?;
    validate_uses(uses)?;
    scan_for_private_subcommands(name)?;
    scan_for_private_subcommands(uses)?;
    for (key, value) in &with {
        crate::expressions::check_with_key(key)?;
        crate::expressions::check_with_value(key, value)?;
        scan_for_private_subcommands(key)?;
        scan_for_private_subcommands(value)?;
    }
    crate::commands::validate_env(&env)?;
    Ok(Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
            env,
        },
    })
}

/// Replace the actionlint header line with the renderer marker.
///
/// The actionlint adapter emits its own header comment while the renderer
/// requires its exact marker on every tree file; the composition boundary
/// normalizes the first line only and passes the body through untouched.
/// # Errors
pub fn rehead_actionlint_marker(yaml: &str, version: &str) -> Result<String, RenderError> {
    let Some((_, body)) = yaml.split_once('\n') else {
        return Err(RenderError::BadCommand(
            "actionlint_without_header".to_owned(),
        ));
    };
    Ok(format!("{}\n{body}", marker::marker_for_version(version)?))
}

/// Fixed script writing the per-target candidate manifest JSON.
///
/// Emits exactly the contract keys: source commit, target triple,
/// toolchain identity, and binary SHA-256 (computed at runtime).
///
/// Contract exception (shell writer, kept deliberately): no trusted
/// executor exists yet in the candidate job — the only Rust binary
/// present is the just-built, still-unverified candidate, which §3
/// verify-before-run forbids running before `candidate_manifest_verify_step`.
#[must_use]
pub fn candidate_manifest_script(target: &str, toolchain: &str) -> String {
    format!(
        "mkdir -p {CANDIDATE_OUTPUT_DIR} && cp target/release/velnor-actions {CANDIDATE_OUTPUT_DIR}/velnor-actions && sha256sum {CANDIDATE_OUTPUT_DIR}/velnor-actions | cut -d' ' -f1 > {CANDIDATE_OUTPUT_DIR}/sha.txt && read sha rest < {CANDIDATE_OUTPUT_DIR}/sha.txt && printf '{{\"schema\":1,\"commit\":\"%s\",\"target\":\"{target}\",\"toolchain\":\"{toolchain}\",\"sha256\":\"%s\"}}' \"$GITHUB_SHA\" \"$sha\" > {CANDIDATE_OUTPUT_DIR}/{CANDIDATE_MANIFEST_FILE}"
    )
}

/// Shell spelling of the run-key dir holding the downloaded plan.
pub const RUN_DIR_SHELL: &str = "$RUNNER_TEMP/velnor/r$GITHUB_RUN_ID-a$GITHUB_RUN_ATTEMPT";

/// Head-bound candidate attestation writer: `commit` is the plan head.
///
/// Reads the downloaded plan's `head` with the manifest-verify idiom
/// (first `"head":"` match, parameter expansion only, no command
/// substitution) and writes `{"schema":1,"commit":"<head>"}` beside
/// the candidate manifest so the same artifact carries it to the
/// final job. Fails closed when the plan is missing or the head is
/// empty/oversize; the merge re-checks `commit == plan.head` after
/// parsing both sides, so a truncation or misparse can only fail a
/// run, never forge a binding.
///
/// Contract exception (shell writer, kept deliberately): same job,
/// same bar — no trusted executor pre-verification, so the plan head
/// is scraped in shell and re-checked by the merge (see above).
#[must_use]
pub fn candidate_attestation_script() -> String {
    let attestation = velnor_actions_contract::CANDIDATE_ATTESTATION_FILENAME;
    format!(
        "line=; rest=; p=\"{RUN_DIR_SHELL}/plan.json\" && test -f \"$p\" && read line rest < \"$p\" || [ -n \"$line\" ] && h=${{line#*\\\"head\\\":\\\"}} && h=${{h%%\\\"*}} && [ -n \"$h\" ] && [ \"${{#h}}\" -le 64 ] && printf '{{\"schema\":1,\"commit\":\"%s\"}}' \"$h\" > {CANDIDATE_OUTPUT_DIR}/{attestation}"
    )
}

/// Acquire-Velnor step: digest-verified staging under runner temp.
///
/// The asset download is an unauthenticated public fetch over a pinned
/// URL, so the step carries the scrub overlay on top of its asset env.
/// # Errors
pub fn acquire_velnor_step(
    argv: Vec<String>,
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    commands::validate_command_argv(&argv)?;
    commands::validate_env(env)?;
    if env
        .get(ASSET_SHA_ENV)
        .is_none_or(|sha| !velnor_actions_contract::ids::is_lower_hex_len(sha, 64))
    {
        return Err(RenderError::BadCommand("bad_asset_sha256".to_owned()));
    }
    if env
        .get(ASSET_URL_ENV)
        .is_none_or(|url| !url.starts_with("https://"))
    {
        return Err(RenderError::BadCommand("bad_asset_url".to_owned()));
    }
    if env
        .get(RELEASE_COMMIT_ENV)
        .is_none_or(|commit| !velnor_actions_contract::ids::is_lower_hex_len(commit, 40))
    {
        return Err(RenderError::BadCommand("bad_release_commit".to_owned()));
    }
    if !argv.iter().any(|arg| arg.contains(STAGED_BINARY_PREFIX)) {
        return Err(RenderError::BadCommand("unstaged_binary".to_owned()));
    }
    let downloads = argv.iter().any(|arg| arg.contains(ASSET_URL_ENV));
    let verifies = argv.iter().any(|arg| arg.contains(ASSET_SHA_ENV));
    if !downloads || !verifies {
        return Err(RenderError::BadCommand("acquire_without_verify".to_owned()));
    }
    let mut step = shell_step(ACQUIRE_NAME, argv, env.clone())?;
    step.role = Some(StepRole::AcquireVelnor);
    Ok(step)
}

/// Target-directory prefix isolating one lane.
pub const TARGET_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/target/";

/// Isolated target directory for one lane.
#[must_use]
pub fn target_dir_for_lane(lane_id: &str) -> String {
    format!("{TARGET_DIR_PREFIX}{lane_id}")
}

/// `CARGO_TARGET_DIR` env pair isolating one lane (CACHE-1.20).
#[must_use]
pub fn lane_cargo_target_env(lane_id: &str) -> (String, String) {
    ("CARGO_TARGET_DIR".to_owned(), target_dir_for_lane(lane_id))
}
