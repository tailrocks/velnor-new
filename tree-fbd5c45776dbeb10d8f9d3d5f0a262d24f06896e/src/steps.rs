//! Fixed step templates over validated command strings.
//!
//! Templates fix names, env keys, and payload shapes; argv arrives validated.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, commands, marker};

pub use crate::cache_steps::{
    CACHE_RESTORE_NAME, CACHE_SAVE_NAME, MBX_ACTION_NAME, TARGET_DIR_PREFIX, TASK_ARTIFACTS_DIR,
    TOOLS_CACHE_PATH, TOOLS_KEY_PREFIX, TOOLS_RESTORE_NAME, TOOLS_RESTORE_USES, TOOLS_SAVE_NAME,
    TOOLS_SAVE_USES, cache_action_step, mbx_objects_step, target_dir_for_lane, tools_cache_key,
    tools_restore_step, tools_save_step,
};

/// Env key selecting the staged-binary internal operation.
pub const INTERNAL_OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Env key carrying the JSON request-file path.
pub const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
/// Planner operation name.
pub const PLAN_OPERATION: &str = "plan-v1";
/// Report-merge operation name.
pub const MERGE_OPERATION: &str = "merge-v1";
/// Write-request operation name.
pub const WRITE_REQUEST_OPERATION: &str = "write-request-v1";
/// Required prefix of the digest-verified staged binary path.
pub const STAGED_BINARY_PREFIX: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-";
/// Required prefix of internal request directories.
pub const REQUEST_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/";
/// Env key carrying the downloaded asset SHA-256.
pub const ASSET_SHA_ENV: &str = "VELNOR_ASSET_SHA256";
/// Env key carrying the downloaded asset URL.
pub const ASSET_URL_ENV: &str = "VELNOR_ASSET_URL";
/// Substrings that must never appear in rendered YAML.
pub const FORBIDDEN_TOKENS: &[&str] = &["__internal", "velnor-actions __", "velnor-actions run"];
/// Pinned `actions/upload-artifact` ref (v7.0.1, qualified 2026-09-28).
pub const UPLOAD_ARTIFACT_USES: &str =
    "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a";
/// Pinned `actions/download-artifact` ref (v8.0.1, qualified 2026-09-28).
pub const DOWNLOAD_ARTIFACT_USES: &str =
    "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c";
/// Candidate artifact name shared by the upload and download steps.
pub const CANDIDATE_ARTIFACT_NAME: &str = "velnor-candidate";
/// Candidate manifest filename inside the uploaded artifact.
pub const CANDIDATE_MANIFEST_FILE: &str = "candidate-manifest.json";
/// Directory holding the built candidate binary plus its manifest.
pub const CANDIDATE_OUTPUT_DIR: &str = "$RUNNER_TEMP/velnor/candidate-output";
/// Directory receiving the downloaded candidate for qualification.
pub const CANDIDATE_STAGE_DIR: &str = "$RUNNER_TEMP/velnor/candidate";
/// Contract-fixed display name of the helper-staging step.
pub const ACQUIRE_NAME: &str = "Acquire Velnor";
/// Run-key expression (`r<run-id>-a<run-attempt>`, workflow-contract §3).
pub const RUN_KEY_EXPR: &str = "r${{ github.run_id }}-a${{ github.run_attempt }}";
/// Display name of the matrix-report upload step.
pub const MATRIX_REPORT_UPLOAD_NAME: &str = "Upload matrix report";
/// Contract-fixed display name of the policy cargo-deny step.
pub const DENY_STEP_NAME: &str = "Run cargo-deny";
/// Contract-fixed display name of the policy cargo-machete step.
pub const MACHETE_STEP_NAME: &str = "Run cargo-machete";

pub use crate::commands::{
    has_bare_env_expansion, quote_env_path_for_run, quote_run_line_env_paths,
};

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

/// Validate an `owner/repo@<40 hex>` action ref; branches are rejected.
/// # Errors
pub fn validate_uses(uses: &str) -> Result<(), RenderError> {
    let Some((name, sha)) = uses.split_once('@') else {
        return Err(RenderError::BadActionRef(format!("missing_sha:{uses}")));
    };
    let Some((owner, repo)) = name.split_once('/') else {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    };
    if owner.is_empty() || repo.is_empty() || !is_action_name(name) {
        return Err(RenderError::BadActionRef(format!("malformed_name:{uses}")));
    }
    if name.starts_with("actions/setup-") || name == "taiki-e/install-action" {
        return Err(RenderError::BadActionRef(format!(
            "forbidden_action:{uses}"
        )));
    }
    if sha.len() != 40 || !is_lower_hex(sha) {
        return Err(RenderError::BadActionRef(format!("unpinned_ref:{uses}")));
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
    Ok(Step {
        name: "Checkout".to_owned(),
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
        },
    })
}

/// Validated pinned-action step.
/// # Errors
pub fn action_step(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_name".to_owned()));
    }
    validate_uses(uses)?;
    scan_for_private_subcommands(name)?;
    scan_for_private_subcommands(uses)?;
    for entry in with.iter().flat_map(|(key, value)| [key, value]) {
        scan_for_private_subcommands(entry)?;
    }
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
        },
    })
}

/// Validated fixed-argv shell step.
/// # Errors
pub fn shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    commands::validate_command_argv(&argv)?;
    commands::validate_env(&env)?;
    scan_for_private_subcommands(name)?;
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Shell { run: argv, env },
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

/// Candidate-artifact upload step over the pinned upload action.
/// # Errors
pub fn upload_artifact_step(name: &str, path: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() || path.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_artifact_io".to_owned()));
    }
    action_step(
        "Upload candidate",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), name.to_owned()),
            ("path".to_owned(), path.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
        ]),
    )
}

/// Candidate-artifact download step over the pinned download action.
/// # Errors
pub fn download_artifact_step(name: &str, path: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() || path.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_artifact_io".to_owned()));
    }
    action_step(
        "Download candidate",
        DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), name.to_owned()),
            ("path".to_owned(), path.to_owned()),
        ]),
    )
}

/// Matrix-report upload step (`velnor-matrix-<run-key>-<matrix-key>`).
///
/// Carries the leg's `matrix-report.json` plus `tasks/` files; the
/// matrix key resolves from the leg's matrix context at runtime, so
/// this step belongs in the matrix task template only. `if: always()`
/// is attached at render.
/// # Errors
pub fn matrix_report_upload_step() -> Result<Step, RenderError> {
    action_step(
        MATRIX_REPORT_UPLOAD_NAME,
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                format!("velnor-matrix-{RUN_KEY_EXPR}-${{{{ matrix.matrix_key }}}}"),
            ),
            (
                "path".to_owned(),
                format!(
                    "${{{{ runner.temp }}}}/velnor/{RUN_KEY_EXPR}/${{{{ matrix.matrix_key }}}}"
                ),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
        ]),
    )
}

/// Fixed script writing the per-target candidate manifest JSON.
///
/// Emits exactly the contract keys: source commit, target triple,
/// toolchain identity, and binary SHA-256 (computed at runtime).
#[must_use]
pub fn candidate_manifest_script(target: &str, toolchain: &str) -> String {
    format!(
        "mkdir -p {CANDIDATE_OUTPUT_DIR} && cp target/release/velnor-actions {CANDIDATE_OUTPUT_DIR}/velnor-actions && sha256sum {CANDIDATE_OUTPUT_DIR}/velnor-actions | cut -d' ' -f1 > {CANDIDATE_OUTPUT_DIR}/sha.txt && read sha rest < {CANDIDATE_OUTPUT_DIR}/sha.txt && printf '{{\"schema\":1,\"commit\":\"%s\",\"target\":\"{target}\",\"toolchain\":\"{toolchain}\",\"sha256\":\"%s\"}}' \"$GITHUB_SHA\" \"$sha\" > {CANDIDATE_OUTPUT_DIR}/{CANDIDATE_MANIFEST_FILE}"
    )
}

/// Acquire-Velnor step: digest-verified staging under runner temp.
/// # Errors
pub fn acquire_velnor_step(
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    commands::validate_command_argv(&argv)?;
    commands::validate_env(&env)?;
    if env
        .get(ASSET_SHA_ENV)
        .is_none_or(|sha| sha.len() != 64 || !is_lower_hex(sha))
    {
        return Err(RenderError::BadCommand("bad_asset_sha256".to_owned()));
    }
    if env
        .get(ASSET_URL_ENV)
        .is_none_or(|url| !url.starts_with("https://"))
    {
        return Err(RenderError::BadCommand("bad_asset_url".to_owned()));
    }
    if !argv.iter().any(|arg| arg.contains(STAGED_BINARY_PREFIX)) {
        return Err(RenderError::BadCommand("unstaged_binary".to_owned()));
    }
    let downloads = argv.iter().any(|arg| arg.contains(ASSET_URL_ENV));
    let verifies = argv.iter().any(|arg| arg.contains(ASSET_SHA_ENV));
    if !downloads || !verifies {
        return Err(RenderError::BadCommand("acquire_without_verify".to_owned()));
    }
    shell_step(ACQUIRE_NAME, argv, env)
}

/// Split an internal operation into env op plus request-file target op.
///
/// `plan-v1`/`merge-v1` target themselves; `write-request-v1:<target>` gates
/// on `write-request-v1` while materializing the target's request file.
/// # Errors
pub(crate) fn split_internal_operation(operation: &str) -> Result<(&str, &str), RenderError> {
    if operation == PLAN_OPERATION || operation == MERGE_OPERATION {
        return Ok((operation, operation));
    }
    let rest = operation
        .strip_prefix(WRITE_REQUEST_OPERATION)
        .and_then(|rest| rest.strip_prefix(':'));
    if let Some(target) = rest
        && (target == PLAN_OPERATION || target == MERGE_OPERATION)
    {
        return Ok((WRITE_REQUEST_OPERATION, target));
    }
    Err(RenderError::BadCommand(format!(
        "unknown_internal_op:{operation}"
    )))
}

/// Internal plan/merge/write-request step; operation travels via env, never argv.
/// # Errors
pub fn internal_step(name: &str, operation: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    split_internal_operation(operation)?;
    scan_for_private_subcommands(name)?;
    Ok(Step {
        name: name.to_owned(),
        kind: StepKind::Internal {
            operation: operation.to_owned(),
        },
    })
}

/// Fixed write-request step materializing `<target>-request.json` at event time.
/// # Errors
pub fn write_request_step(target: &str) -> Result<Step, RenderError> {
    if target != PLAN_OPERATION && target != MERGE_OPERATION {
        return Err(RenderError::BadCommand(format!(
            "unknown_internal_op:{target}"
        )));
    }
    Ok(Step {
        name: "Write request".to_owned(),
        kind: StepKind::Internal {
            operation: format!("{WRITE_REQUEST_OPERATION}:{target}"),
        },
    })
}

/// Fixed planner step (`plan-v1`).
#[must_use]
pub fn plan_step() -> Step {
    Step {
        name: "Plan".to_owned(),
        kind: StepKind::Internal {
            operation: PLAN_OPERATION.to_owned(),
        },
    }
}

/// Fixed report-merge step (`merge-v1`).
#[must_use]
pub fn merge_step() -> Step {
    Step {
        name: "Merge reports".to_owned(),
        kind: StepKind::Internal {
            operation: MERGE_OPERATION.to_owned(),
        },
    }
}

/// True for `owner/repo` over alphanumerics plus `.-_`.
fn is_action_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
}

/// True for 64-char lowercase hex.
fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
