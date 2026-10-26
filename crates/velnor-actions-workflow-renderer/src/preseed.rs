// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepRole};

use crate::{
    RenderError,
    artifact_paths::{PRESEED_OUTPUT_DIR_EXPR, PRESEED_STAGE_DIR_EXPR},
    cache_steps::RESOLVE_ACTION_MBX_BIN,
    steps,
};

#[path = "preseed_mbx.rs"]
mod mbx;

pub const PRESEED_ARTIFACT_NAME: &str = "velnor-preseed-helper";
pub const PRESEED_STAGE_DIR: &str = "$RUNNER_TEMP/velnor/preseed";
pub const PRESEED_MANIFEST_FILE: &str = "preseed-manifest.json";
pub const PRESEED_BUILD_OUTPUT: &str = "target/release/velnor-actions";
const PRESEED_MBX_VERSION_OUTPUT: &str = "$RUNNER_TEMP/velnor/preseed-mbx-version";
pub const PRESEED_DOWNLOADED_BINARY: &str = "$RUNNER_TEMP/velnor/preseed/velnor-actions";

const TRUST_MARK: &str = " (pre-seed trust-on-review)";
pub const PRESEED_BUILD_NAME: &str = "Build helper (pre-seed trust-on-review)";
pub const PRESEED_VERIFY_NAME: &str = "Verify MBX compile (pre-seed trust-on-review)";
pub const PRESEED_MANIFEST_NAME: &str = "Write helper manifest (pre-seed trust-on-review)";
pub const PRESEED_UPLOAD_NAME: &str = "Upload helper (pre-seed trust-on-review)";
pub const PRESEED_DOWNLOAD_NAME: &str = "Download helper (pre-seed trust-on-review)";
pub const PRESEED_VERIFY_MANIFEST_NAME: &str = "Verify helper manifest (pre-seed trust-on-review)";
pub const PRESEED_STAGE_NAME: &str = "Stage helper (pre-seed trust-on-review)";
pub const PRESEED_MANIFEST_BINARY_ENV: &str = "VELNOR_PRESEED_BINARY";
pub const PRESEED_MANIFEST_OUT_ENV: &str = "VELNOR_PRESEED_OUT";
pub const PRESEED_MANIFEST_TARGET_ENV: &str = "VELNOR_PRESEED_TARGET";
pub const PRESEED_MANIFEST_TOOLCHAIN_ENV: &str = "VELNOR_PRESEED_TOOLCHAIN";

pub(crate) fn toolchain_identity(build: &[String]) -> Result<String, RenderError> {
    let mut specs = Vec::new();
    let mut in_specs = false;
    for arg in build {
        if arg == "exec" {
            in_specs = true;
        } else if arg == "--" {
            break;
        } else if in_specs {
            specs.push(arg.clone());
        }
    }
    if specs.is_empty() || specs.iter().any(|spec| !spec.contains('@')) {
        return Err(RenderError::BadCommand(
            "candidate_build_without_toolchain".to_owned(),
        ));
    }
    Ok(specs.join("+"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreseedStageSource {
    LocalBuild,
    DownloadedArtifact,
}

impl PreseedStageSource {
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::LocalBuild => PRESEED_BUILD_OUTPUT,
            Self::DownloadedArtifact => PRESEED_DOWNLOADED_BINARY,
        }
    }
}

pub fn preseed_build_step(
    build: &[String],
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_BUILD_NAME.ends_with(TRUST_MARK));
    let build = mbx::build_argv_for_action(build)?;
    let command = crate::commands::join_argv_for_run(&build)?;
    let script = format!("set -eu; {RESOLVE_ACTION_MBX_BIN}; {command}");
    let mut step = steps::action_owned_mbx_shell_step(
        PRESEED_BUILD_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env.clone(),
    )?;
    step.role = Some(StepRole::PreseedBuild);
    Ok(step)
}

pub fn preseed_verify_step(
    probe: &[String],
    mbx_version: &str,
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_VERIFY_NAME.ends_with(TRUST_MARK));
    if !crate::commands::is_exact_or_project_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "preseed_bad_mbx_version:{mbx_version}"
        )));
    }
    let probe = mbx::probe_argv_for_action(probe)?;
    let command = crate::commands::join_argv_for_run(&probe)?;
    let script = format!(
        "set -eu; {RESOLVE_ACTION_MBX_BIN}; test -x {PRESEED_BUILD_OUTPUT} && mkdir -p \"$RUNNER_TEMP/velnor\" && {command} > \"{PRESEED_MBX_VERSION_OUTPUT}\" && grep -qxF \"mbx {mbx_version}\" \"{PRESEED_MBX_VERSION_OUTPUT}\""
    );
    let mut step = steps::action_owned_mbx_shell_step(
        PRESEED_VERIFY_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env.clone(),
    )?;
    step.role = Some(StepRole::PreseedVerifyBuild);
    Ok(step)
}

fn check_probe_shape(probe: &[String]) -> Result<(), RenderError> {
    let malformed = || RenderError::BadCommand("preseed_bad_mbx_probe".to_owned());
    if probe.len() != 9
        || probe.iter().any(|arg| {
            !is_probe_token(
                &velnor_actions_contract::config::project_tool_refs::without_project_tool_refs(arg),
            )
        })
        || probe.iter().take(5).map(String::as_str).ne([
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
        ])
        || probe
            .iter()
            .skip(6)
            .map(String::as_str)
            .ne(["--", "mbx", "--version"])
    {
        return Err(malformed());
    }
    let Some(rust_spec) = probe[5].strip_prefix("rust@") else {
        return Err(malformed());
    };
    if !crate::commands::is_exact_or_project_version(rust_spec) {
        return Err(malformed());
    }
    Ok(())
}

fn is_probe_token(arg: &str) -> bool {
    !arg.is_empty()
        && arg.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@' | b':' | b'/' | b'+')
        })
}

pub fn preseed_manifest_step(build: &[String], target: &str) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_MANIFEST_NAME.ends_with(TRUST_MARK));
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "preseed_unsupported_target:{target}"
        )));
    }
    let toolchain = toolchain_identity(build)?;
    let mut step = steps::shell_step(
        PRESEED_MANIFEST_NAME,
        vec![PRESEED_BUILD_OUTPUT.to_owned()],
        std::collections::BTreeMap::from([
            (
                steps::INTERNAL_OP_ENV.to_owned(),
                steps::WRITE_PRESEED_MANIFEST_OPERATION.to_owned(),
            ),
            (
                PRESEED_MANIFEST_BINARY_ENV.to_owned(),
                PRESEED_BUILD_OUTPUT.to_owned(),
            ),
            (
                PRESEED_MANIFEST_OUT_ENV.to_owned(),
                PRESEED_OUTPUT_DIR_EXPR.to_owned(),
            ),
            (PRESEED_MANIFEST_TARGET_ENV.to_owned(), target.to_owned()),
            (PRESEED_MANIFEST_TOOLCHAIN_ENV.to_owned(), toolchain),
        ]),
    )?;
    step.role = Some(StepRole::PreseedManifest);
    Ok(step)
}

pub fn preseed_upload_step() -> Result<Step, RenderError> {
    debug_assert!(PRESEED_UPLOAD_NAME.ends_with(TRUST_MARK));
    let mut step = steps::action_step(
        PRESEED_UPLOAD_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PRESEED_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PRESEED_OUTPUT_DIR_EXPR.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                steps::ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::PreseedUpload);
    Ok(step)
}

pub fn preseed_download_step() -> Result<Step, RenderError> {
    debug_assert!(PRESEED_DOWNLOAD_NAME.ends_with(TRUST_MARK));
    let mut step = steps::action_step(
        PRESEED_DOWNLOAD_NAME,
        steps::DOWNLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PRESEED_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PRESEED_STAGE_DIR_EXPR.to_owned()),
        ]),
    )?;
    step.role = Some(StepRole::PreseedDownload);
    Ok(step)
}

#[must_use]
pub fn preseed_manifest_verify_script(target: &str) -> String {
    format!(
        "line=; rest=; m=\"{PRESEED_STAGE_DIR}/{PRESEED_MANIFEST_FILE}\" && b=\"{PRESEED_STAGE_DIR}/velnor-actions\" && test -f \"$m\" && test -f \"$b\" && read line rest < \"$m\" || [ -n \"$line\" ] && v=${{line#*\\\"schema\\\":}} && v=${{v%%,*}} && [ \"$v\" = 1 ] && c=${{line#*\\\"commit\\\":\\\"}} && c=${{c%%\\\"*}} && [ \"${{#c}}\" = 40 ] && [ \"$c\" = \"$GITHUB_SHA\" ] && t=${{line#*\\\"target\\\":\\\"}} && t=${{t%%\\\"*}} && [ \"$t\" = \"{target}\" ] && tc=${{line#*\\\"toolchain\\\":\\\"}} && tc=${{tc%%\\\"*}} && [ -n \"$tc\" ] && s=${{line#*\\\"sha256\\\":\\\"}} && s=${{s%%\\\"*}} && [ \"${{#s}}\" = 64 ] && sha256sum \"$b\" > \"{PRESEED_STAGE_DIR}/got.txt\" && read got rest < \"{PRESEED_STAGE_DIR}/got.txt\" && [ \"$got\" = \"$s\" ]"
    )
}

pub fn preseed_manifest_verify_step(target: &str) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_VERIFY_MANIFEST_NAME.ends_with(TRUST_MARK));
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "preseed_unsupported_target:{target}"
        )));
    }
    let mut step = steps::shell_step(
        PRESEED_VERIFY_MANIFEST_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            preseed_manifest_verify_script(target),
        ],
        std::collections::BTreeMap::new(),
    )?;
    step.role = Some(StepRole::PreseedVerifyManifest);
    Ok(step)
}

pub fn preseed_stage_step(source: PreseedStageSource, staged: &str) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_STAGE_NAME.ends_with(TRUST_MARK));
    validate_staged_path(staged)?;
    let dir = staged.rsplit_once('/').map_or(staged, |(head, _)| head);
    let script = format!(
        "mkdir -p {dir} && cp {} {staged} && chmod +x {staged}",
        source.path()
    );
    let mut step = steps::shell_step(
        PRESEED_STAGE_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        std::collections::BTreeMap::new(),
    )?;
    step.role = Some(StepRole::PreseedStage);
    Ok(step)
}

fn validate_staged_path(staged: &str) -> Result<(), RenderError> {
    match staged.strip_prefix(steps::STAGED_BINARY_PREFIX) {
        Some(rest)
            if !rest.is_empty()
                && !rest.split('/').any(|seg| seg.is_empty() || seg == "..")
                && !rest.chars().any(|ch| ch.is_whitespace() || ch.is_control())
                && !rest.contains("${{") =>
        {
            Ok(())
        }
        _ => Err(RenderError::BadCommand(format!(
            "preseed_unstaged_binary:{staged}"
        ))),
    }
}
