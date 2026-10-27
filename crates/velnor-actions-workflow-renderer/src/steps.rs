// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_actionlint::{
    CHECKOUT_ACTION_USES, DOWNLOAD_ARTIFACT_ACTION_USES, UPLOAD_ARTIFACT_ACTION_USES,
};

pub const ATTEST_BUILD_PROVENANCE_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
pub const CREATE_GITHUB_APP_TOKEN_USES: &str =
    "actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510";
use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, commands, marker};

pub use crate::cache_steps::{
    CACHE_RESTORE_NAME, CACHE_SAVE_NAME, CompileDriver, FAMILY_KEYS_NAME, FamilyCachePlan,
    FamilySnapshot, MBX_ACTION_NAME, MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_IMPORT_NAME,
    MBX_BUNDLE_KEY_NAME, MBX_BUNDLE_RESTORE_NAME, MBX_BUNDLE_SAVE_NAME, MBX_EXPORT_GROUP_ENV,
    MBX_EXPORT_GROUP_VALUE, MBX_PREFLIGHT_NAME, MBX_PRODUCER_MARK_NAME, MBX_RESTORE_NAME,
    MBX_VERSION_CHECK_NAME, MbxTransport, NEVER_ARCHIVE_MARKERS, TASK_ARTIFACTS_DIR,
    TOOLS_CACHE_PATH, TOOLS_CACHE_PATHS, TOOLS_RESTORE_ACTION_USES, TOOLS_RESTORE_NAME,
    TOOLS_RESTORE_USES, TOOLS_SAVE_NAME, TOOLS_SAVE_USES, cache_action_step,
    check_cache_step_order, check_mbx_gating, family_keys_step, family_keys_step_single_writer,
    family_restore_step, family_save_step, family_size_step, is_never_archive_path,
    mbx_producer_mark_step, mbx_steps_for_driver,
};
pub(crate) use crate::cache_steps::{tools_cache_restore_step, tools_cache_save_step};
pub use crate::parallel_target_seed::seed_parallel_targets_step;
pub(crate) use crate::steps_shell::action_owned_mbx_shell_step;
pub(crate) use crate::steps_shell::composite_shell_step;
pub use crate::steps_shell::{ambient_shell_step, jactionlint_verifier_step, shell_step};

pub use crate::action_ref::validate_uses;
pub use crate::steps_artifact::{
    crate_job_report_upload_step, download_artifact_step, download_suite_archive_step,
    matrix_report_upload_step, upload_artifact_step, upload_suite_archive_step,
    upload_suite_artifact_step,
};
pub(crate) use crate::steps_internal::split_internal_operation;
pub use crate::steps_internal::{internal_step, merge_step, plan_step, write_request_step};

pub const INTERNAL_OP_ENV: &str = "VELNOR_INTERNAL_OP";
pub const REQUEST_FILE_ENV: &str = "VELNOR_REQUEST_FILE";
pub const PLAN_OPERATION: &str = "plan-v1";
pub const WRITE_PRESEED_MANIFEST_OPERATION: &str = "write-preseed-manifest-v1";
pub const MERGE_OPERATION: &str = "merge-v1";
pub const FETCH_OPERATION: &str = "fetch-reports-v1";
pub const WRITE_REQUEST_OPERATION: &str = "write-request-v1";
pub const RESOLVE_QUALIFICATION_OPERATION: &str = "resolve-qualification-v1";
pub const STAGED_BINARY_PREFIX: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-";
pub const REQUEST_DIR_PREFIX: &str = "${{ runner.temp }}/velnor/";
pub const ASSET_SHA_ENV: &str = "VELNOR_ASSET_SHA256";
pub const ASSET_URL_ENV: &str = "VELNOR_ASSET_URL";
pub const RELEASE_COMMIT_ENV: &str = "VELNOR_RELEASE_COMMIT";
pub const FORBIDDEN_TOKENS: &[&str] = &["__internal", "velnor-actions __", "velnor-actions run"];
pub const UPLOAD_ARTIFACT_USES: &str = UPLOAD_ARTIFACT_ACTION_USES;
pub const CHECKOUT_USES: &str = CHECKOUT_ACTION_USES;
pub const ARTIFACT_RETENTION_DAYS: u32 = 30;
pub const DOWNLOAD_ARTIFACT_USES: &str = DOWNLOAD_ARTIFACT_ACTION_USES;
pub const ACQUIRE_NAME: &str = "Acquire Velnor";

pub const ACQUIRE_TIMEOUT_MINUTES: i64 =
    velnor_actions_contract::config::RELEASE_ACQUIRE_STEP_TIMEOUT_MINUTES as i64;
pub const RUN_KEY_EXPR: &str = "r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}";
pub const MATRIX_REPORT_UPLOAD_NAME: &str = "Upload matrix report";
pub const CRATE_REPORT_UPLOAD_NAME: &str = "Upload crate reports";
pub const DENY_STEP_NAME: &str = "Run cargo-deny";
pub const MACHETE_STEP_NAME: &str = "Run cargo-machete";

pub fn scan_for_private_subcommands(text: &str) -> Result<(), RenderError> {
    for token in FORBIDDEN_TOKENS {
        if text.contains(token) {
            return Err(RenderError::PrivateSubcommand((*token).to_owned()));
        }
    }
    Ok(())
}

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

pub fn action_step(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    action_step_with_env(name, uses, with, BTreeMap::new())
}

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
        parallel: false,
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

pub fn rehead_lint_config_marker(yaml: &str, version: &str) -> Result<String, RenderError> {
    let Some((_, body)) = yaml.split_once('\n') else {
        return Err(RenderError::BadCommand(
            "lint_config_without_header".to_owned(),
        ));
    };
    Ok(format!("{}\n{body}", marker::marker_for_version(version)?))
}

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

pub const CACHE_BOOTSTRAP_SCRIPT: &str = include_str!("cache_bootstrap.py");

pub const BOOTSTRAP_STEP_NAME: &str = "Restore pinned controller";
pub const BOOTSTRAP_MISS_CODE: i32 = 10;
pub const BOOTSTRAP_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const BOOTSTRAP_MARKER_SUFFIX: &str = ".bootstrapped";

pub const BOOTSTRAP_PAYLOAD_DIR_ENV: &str = "VELNOR_BOOTSTRAP_PAYLOAD_DIR";
pub const BOOTSTRAP_MEMBER_ENV: &str = "VELNOR_BOOTSTRAP_MEMBER";
pub const BOOTSTRAP_EXPECTED_SHA_ENV: &str = "VELNOR_BOOTSTRAP_EXPECTED_SHA256";
pub const BOOTSTRAP_STAGED_PATH_ENV: &str = "VELNOR_BOOTSTRAP_STAGED_PATH";
pub const BOOTSTRAP_MAX_BYTES_ENV: &str = "VELNOR_BOOTSTRAP_MAX_BYTES";
pub const BOOTSTRAP_EXPECTED_KEY_ENV: &str = "VELNOR_BOOTSTRAP_EXPECTED_KEY";
pub const BOOTSTRAP_MATCHED_KEY_ENV: &str = "VELNOR_BOOTSTRAP_MATCHED_KEY";
pub const BOOTSTRAP_SHADOW_ENV: &str = "VELNOR_BOOTSTRAP_SHADOW_STATE";
pub const BOOTSTRAP_SHADOW_CLEAR: &str = "clear";

const BOOTSTRAP_MAX_MEMBER_LEN: usize = 256;
const BOOTSTRAP_MAX_LIMIT: u64 = 2_147_483_647;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapBridge {
    pub payload_dir: String,
    pub member: String,
    pub staged_path: String,
    pub max_bytes: u64,
    pub expected_key: Option<String>,
    pub matched_key: Option<String>,
    pub shadow_state: Option<String>,
}

fn bootstrap_member_part_ok(part: &str) -> bool {
    if part.is_empty() || part == "." || part == ".." {
        return false;
    }
    part.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.' || byte == b'_')
}

fn check_bootstrap_member(member: &str) -> Result<(), RenderError> {
    if member.is_empty()
        || member.len() > BOOTSTRAP_MAX_MEMBER_LEN
        || member.starts_with('/')
        || !member.split('/').all(bootstrap_member_part_ok)
    {
        return Err(RenderError::BadCommand("bootstrap_member".to_owned()));
    }
    Ok(())
}

fn check_bootstrap_path(value: &str, problem: &str) -> Result<(), RenderError> {
    if value.is_empty()
        || value.contains('\0')
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('"')
        || value.contains('`')
        || value.contains('\\')
        || value.contains("$(")
    {
        return Err(RenderError::BadCommand(problem.to_owned()));
    }
    if !(value.starts_with('/')
        || value.starts_with("$RUNNER_TEMP")
        || value.starts_with("${{ runner.temp }}"))
    {
        return Err(RenderError::BadCommand(problem.to_owned()));
    }
    Ok(())
}

fn check_bootstrap_bridge(bridge: &BootstrapBridge) -> Result<(), RenderError> {
    check_bootstrap_path(&bridge.payload_dir, "bootstrap_payload_dir")?;
    check_bootstrap_member(&bridge.member)?;
    check_bootstrap_path(&bridge.staged_path, "bootstrap_staged_path")?;
    if bridge.max_bytes == 0 || bridge.max_bytes > BOOTSTRAP_MAX_LIMIT {
        return Err(RenderError::BadCommand("bootstrap_max_bytes".to_owned()));
    }
    match (&bridge.expected_key, &bridge.matched_key) {
        (None, None) => Ok(()),
        (Some(expected), Some(_)) if !expected.is_empty() => Ok(()),
        _ => Err(RenderError::BadCommand("bootstrap_entry_key".to_owned())),
    }?;
    if bridge.shadow_state.as_ref().is_some_and(|state| {
        state.is_empty() || state.len() > 64 || state.contains(char::is_whitespace)
    }) {
        return Err(RenderError::BadCommand("bootstrap_shadow_state".to_owned()));
    }
    Ok(())
}

#[must_use]
pub fn bootstrap_marker_for(staged_path: &str) -> String {
    format!("{staged_path}{BOOTSTRAP_MARKER_SUFFIX}")
}

pub(crate) fn escape_bootstrap_for_printf(script: &str) -> Result<String, RenderError> {
    if script.contains('\r') || script.contains('\0') {
        return Err(RenderError::BadCommand("bootstrap_script_shape".to_owned()));
    }
    let mut out = String::with_capacity(script.len());
    for ch in script.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("'\\''"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    Ok(out)
}

pub fn bootstrap_bridge_script(bridge: &BootstrapBridge) -> Result<String, RenderError> {
    check_bootstrap_bridge(bridge)?;
    let embedded = escape_bootstrap_for_printf(CACHE_BOOTSTRAP_SCRIPT)?;
    let marker = bootstrap_marker_for(&bridge.staged_path);
    Ok(format!(
        "d=\"{staged}\"&&p=\"{payload}\"&&export {staged_env}=\"$d\" {payload_env}=\"$p\"&&printf '%b' '{embedded}' | python3 -;c=$?;if [ \"$c\" = 0 ];then : > \"{marker}\";elif [ \"$c\" = {miss} ];then rm -f \"{marker}\";else exit \"$c\";fi",
        staged = bridge.staged_path,
        payload = bridge.payload_dir,
        staged_env = BOOTSTRAP_STAGED_PATH_ENV,
        payload_env = BOOTSTRAP_PAYLOAD_DIR_ENV,
        miss = BOOTSTRAP_MISS_CODE,
    ))
}

pub fn bootstrap_bridge_step(
    bridge: &BootstrapBridge,
    expected_sha256: &str,
) -> Result<Step, RenderError> {
    check_bootstrap_bridge(bridge)?;
    if !velnor_actions_contract::ids::is_lower_hex_len(expected_sha256, 64) {
        return Err(RenderError::BadCommand("bootstrap_sha256".to_owned()));
    }
    let mut env = BTreeMap::from([
        (BOOTSTRAP_MEMBER_ENV.to_owned(), bridge.member.clone()),
        (
            BOOTSTRAP_EXPECTED_SHA_ENV.to_owned(),
            expected_sha256.to_owned(),
        ),
        (
            BOOTSTRAP_MAX_BYTES_ENV.to_owned(),
            bridge.max_bytes.to_string(),
        ),
    ]);
    if let Some(expected) = &bridge.expected_key {
        env.insert(BOOTSTRAP_EXPECTED_KEY_ENV.to_owned(), expected.clone());
    }
    if let Some(matched) = &bridge.matched_key {
        env.insert(BOOTSTRAP_MATCHED_KEY_ENV.to_owned(), matched.clone());
    }
    if let Some(shadow) = &bridge.shadow_state {
        env.insert(BOOTSTRAP_SHADOW_ENV.to_owned(), shadow.clone());
    }
    let script = bootstrap_bridge_script(bridge)?;
    shell_step(
        BOOTSTRAP_STEP_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )
}

pub fn bootstrap_guarded_acquire_argv(
    marker: &str,
    acquire_argv: &[String],
) -> Result<Vec<String>, RenderError> {
    let body = match acquire_argv {
        [shell, flag, body]
            if (shell == "sh" || shell == "bash" || shell == "/bin/sh")
                && flag == "-c"
                && !body.trim().is_empty() =>
        {
            body
        }
        _ => {
            return Err(RenderError::BadCommand(
                "bootstrap_acquire_shape".to_owned(),
            ));
        }
    };
    commands::validate_command_argv(acquire_argv)?;
    Ok(vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("if [ -f \"{marker}\" ]; then exit 0; else {body}; fi"),
    ])
}

pub const TARGET_DIR_PREFIX: &str = "$RUNNER_TEMP/velnor/target/";

#[must_use]
pub fn target_dir_for_lane(lane_id: &str) -> String {
    format!("{TARGET_DIR_PREFIX}{lane_id}")
}

#[must_use]
pub fn lane_cargo_target_env(lane_id: &str) -> (String, String) {
    ("CARGO_TARGET_DIR".to_owned(), target_dir_for_lane(lane_id))
}
