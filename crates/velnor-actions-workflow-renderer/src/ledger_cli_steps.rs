// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ledger_cli::{
    LEDGER_CLI_ARTIFACT_NAME, LEDGER_CLI_ARTIFACT_NAME_EXPR, LEDGER_CLI_BUILD_STEP,
    LEDGER_CLI_CONSUME_CONDITION, LEDGER_CLI_DOWNLOAD_STEP, LEDGER_CLI_FEATURE,
    LEDGER_CLI_FEATURES_FLAG, LEDGER_CLI_NO_DEFAULT_FEATURES_FLAG, LEDGER_CLI_OFFLINE_FLAG,
    LEDGER_CLI_PUBLISH_STEP, LEDGER_CLI_REJECTION_MARKER, LEDGER_CLI_SHA256_ENV,
    LEDGER_CLI_SHA256_EXPR, LEDGER_CLI_SMOKE_OP, LEDGER_CLI_SOURCE_SHA_ENV,
    LEDGER_CLI_SOURCE_SHA_EXPR, LEDGER_CLI_UPLOAD_STEP, LEDGER_CLI_VERIFY_STEP,
};
use velnor_actions_contract::{
    RUSTUP_AUTO_INSTALL_DISABLED, RUSTUP_AUTO_INSTALL_ENV, Step, StepId, StepRole,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::yaml::Yaml;
use crate::{RenderError, commands, steps};

pub const LEDGER_CLI_TARGET_DIR: &str = "${{ runner.temp }}/velnor/ledger-cli-target";

const BINARY_NAME: &str = "velnor-actions";

const DOWNLOAD_PATH: &str = "${{ runner.temp }}/velnor/ledger-cli";

const UPLOAD_PATH: &str = "${{ runner.temp }}/velnor/ledger-cli-out/velnor-actions";

const TARGET_ENV: &str = "CARGO_TARGET_DIR";

const LEAN_BUILD_ENV: [(&str, &str); 3] = [
    ("CARGO_INCREMENTAL", "0"),
    ("CARGO_PROFILE_DEV_DEBUG", "0"),
    ("CARGO_PROFILE_DEV_STRIP", "symbols"),
];

fn verify_script() -> String {
    format!(
        concat!(
            "set -eu; PATH=\"{}\"; export PATH; ",
            "f=\"$RUNNER_TEMP/velnor/ledger-cli/velnor-actions\"; ",
            "[ -f \"$f\" ] && [ ! -L \"$f\" ]; ",
            "if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$f\"; else shasum -a 256 \"$f\"; fi | ",
            "{{ read -r actual _; [ -n \"$VELNOR_LEDGER_CLI_SHA256\" ] && [ \"$actual\" = \"$VELNOR_LEDGER_CLI_SHA256\" ]; }}; ",
            "git rev-parse HEAD | {{ read -r head; [ -n \"$VELNOR_LEDGER_CLI_SOURCE_SHA\" ] && [ \"$head\" = \"$VELNOR_LEDGER_CLI_SOURCE_SHA\" ]; }}; ",
            "chmod +x \"$f\"",
        ),
        crate::VERIFY_PATH,
    )
}

fn publish_script() -> String {
    format!(
        concat!(
            "set -eu; ",
            "bin=\"$CARGO_TARGET_DIR/debug/velnor-actions\"; ",
            "smoke=\"$RUNNER_TEMP/velnor/ledger-cli-smoke.txt\"; ",
            "mkdir -p \"$RUNNER_TEMP/velnor\"; ",
            "code=0; env -i VELNOR_INTERNAL_OP={} \"$bin\" > \"$smoke\" 2>&1 || code=$?; ",
            "[ \"$code\" -eq 2 ] || exit 1; ",
            "grep -qF \"{}\" \"$smoke\" || exit 1; ",
            "rm -f \"$smoke\"; ",
            "out=\"$RUNNER_TEMP/velnor/ledger-cli-out\"; ",
            "meta=\"$RUNNER_TEMP/velnor/ledger-cli-meta\"; ",
            "rm -rf \"$out\" \"$meta\"; ",
            "mkdir -p \"$out\" \"$meta\"; ",
            "cp \"$bin\" \"$out/velnor-actions\"; ",
            "if command -v sha256sum >/dev/null 2>&1; then ",
            "sha256sum \"$out/velnor-actions\" > \"$meta/sha256\"; ",
            "else ",
            "shasum -a 256 \"$out/velnor-actions\" > \"$meta/sha256\"; ",
            "fi; ",
            "read -r digest _ < \"$meta/sha256\"; ",
            "git rev-parse HEAD > \"$meta/head\"; ",
            "read -r head < \"$meta/head\"; ",
            "printf 'sha256=%s\\nsource_sha=%s\\n' \"$digest\" \"$head\" >> \"$GITHUB_OUTPUT\"",
        ),
        LEDGER_CLI_SMOKE_OP, LEDGER_CLI_REJECTION_MARKER,
    )
}

fn sh_step(
    name: &str,
    script: &str,
    env: BTreeMap<String, String>,
    role: StepRole,
    id: Option<StepId>,
) -> Result<Step, RenderError> {
    let mut step = steps::shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
        env,
    )?;
    step.role = Some(role);
    step.id = id;
    Ok(step)
}

pub fn ledger_cli_build_step(
    catalog: &ToolCatalog,
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    let argv = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        catalog.tool_spec(PinnedTool::Rust).as_str(),
        "--",
        "cargo",
        "build",
        "--locked",
        LEDGER_CLI_OFFLINE_FLAG,
        "-p",
        "velnor-actions-cli",
        LEDGER_CLI_NO_DEFAULT_FEATURES_FLAG,
        LEDGER_CLI_FEATURES_FLAG,
        LEDGER_CLI_FEATURE,
        "--bin",
        BINARY_NAME,
    ]
    .map(str::to_owned)
    .to_vec();
    let mut env = env.clone();
    env.insert(TARGET_ENV.to_owned(), LEDGER_CLI_TARGET_DIR.to_owned());
    env.extend(LEAN_BUILD_ENV.map(|(key, value)| (key.to_owned(), value.to_owned())));
    env.insert(
        RUSTUP_AUTO_INSTALL_ENV.to_owned(),
        RUSTUP_AUTO_INSTALL_DISABLED.to_owned(),
    );
    let mut step = steps::shell_step(LEDGER_CLI_BUILD_STEP, argv, env)?;
    step.role = Some(StepRole::LedgerCliBuild);
    Ok(step)
}

pub fn ledger_cli_publish_step() -> Result<Step, RenderError> {
    sh_step(
        LEDGER_CLI_PUBLISH_STEP,
        &publish_script(),
        BTreeMap::from([(TARGET_ENV.to_owned(), LEDGER_CLI_TARGET_DIR.to_owned())]),
        StepRole::LedgerCliPublish,
        Some(StepId::LedgerCliPublish),
    )
}

pub fn ledger_cli_upload_step() -> Result<Step, RenderError> {
    let mut step = steps::action_step(
        LEDGER_CLI_UPLOAD_STEP,
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), LEDGER_CLI_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), UPLOAD_PATH.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                steps::ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::LedgerCliUpload);
    Ok(step)
}

pub fn ledger_cli_download_step() -> Result<Step, RenderError> {
    let mut step = steps::action_step(
        LEDGER_CLI_DOWNLOAD_STEP,
        steps::DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), LEDGER_CLI_ARTIFACT_NAME_EXPR.to_owned()),
            ("path".to_owned(), DOWNLOAD_PATH.to_owned()),
        ]),
    )?;
    step.role = Some(StepRole::LedgerCliDownload);
    step.condition = Some(LEDGER_CLI_CONSUME_CONDITION.to_owned());
    Ok(step)
}

pub fn ledger_cli_verify_step() -> Result<Step, RenderError> {
    let mut step = sh_step(
        LEDGER_CLI_VERIFY_STEP,
        &verify_script(),
        BTreeMap::from([
            (
                LEDGER_CLI_SHA256_ENV.to_owned(),
                LEDGER_CLI_SHA256_EXPR.to_owned(),
            ),
            (
                LEDGER_CLI_SOURCE_SHA_ENV.to_owned(),
                LEDGER_CLI_SOURCE_SHA_EXPR.to_owned(),
            ),
        ]),
        StepRole::LedgerCliVerify,
        Some(StepId::LedgerCliVerify),
    )?;
    step.condition = Some(LEDGER_CLI_CONSUME_CONDITION.to_owned());
    Ok(step)
}

pub(crate) fn render_verify_step(
    step: &Step,
    run: &[String],
    env: &BTreeMap<String, String>,
    job_env: &BTreeMap<String, String>,
) -> Result<Yaml, RenderError> {
    commands::validate_command_argv(run)?;
    commands::validate_env(env)?;
    let script = match run {
        [sh, flag, script] if sh == "sh" && flag == "-c" => script,
        _ => {
            return Err(RenderError::InvalidWorkflow(
                "ledger_cli_verify_shape".to_owned(),
            ));
        }
    };
    let mut entries = vec![("name".to_owned(), Yaml::str(step.name.clone()))];
    crate::step_ids::push_step_id(&mut entries, step);
    if let Some(condition) = &step.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    let step_env: Vec<(String, Yaml)> = env
        .iter()
        .filter(|(key, value)| job_env.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
        .collect();
    if !step_env.is_empty() {
        entries.push(("env".to_owned(), Yaml::Map(step_env)));
    }
    entries.push(("shell".to_owned(), Yaml::str(crate::VERIFY_VERIFIER_SHELL)));
    entries.push(("run".to_owned(), Yaml::str(script.clone())));
    Ok(Yaml::Map(entries))
}
