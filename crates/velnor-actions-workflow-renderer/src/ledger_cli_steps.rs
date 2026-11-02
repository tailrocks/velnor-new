// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::ledger_cli::{
    LEDGER_CLI_ARTIFACT_NAME, LEDGER_CLI_ARTIFACT_NAME_EXPR, LEDGER_CLI_AUTO_INSTALL_ENV,
    LEDGER_CLI_BUILD_STEP, LEDGER_CLI_CONSUME_CONDITION, LEDGER_CLI_DOWNLOAD_STEP,
    LEDGER_CLI_OFFLINE_FLAG, LEDGER_CLI_PUBLISH_STEP, LEDGER_CLI_RETENTION_DAYS,
    LEDGER_CLI_SHA256_ENV, LEDGER_CLI_SHA256_EXPR, LEDGER_CLI_SOURCE_SHA_ENV,
    LEDGER_CLI_SOURCE_SHA_EXPR, LEDGER_CLI_UPLOAD_STEP, LEDGER_CLI_VERIFY_STEP,
};
use velnor_actions_contract::{Step, StepId, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::{RenderError, steps};

pub const LEDGER_CLI_TARGET_DIR: &str = "${{ runner.temp }}/velnor/ledger-cli-target";

const BINARY_NAME: &str = "velnor-actions";

const DOWNLOAD_PATH: &str = "${{ runner.temp }}/velnor/ledger-cli";

const UPLOAD_PATH: &str = "${{ runner.temp }}/velnor/ledger-cli-out/velnor-actions";

const TARGET_ENV: &str = "CARGO_TARGET_DIR";

const VERIFY_SCRIPT: &str = concat!(
    "set -eu; ",
    "f=\"$RUNNER_TEMP/velnor/ledger-cli/velnor-actions\"; ",
    "[ -f \"$f\" ] && [ ! -L \"$f\" ]; ",
    "if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$f\"; else shasum -a 256 \"$f\"; fi | ",
    "{ read -r actual _; [ -n \"$VELNOR_LEDGER_CLI_SHA256\" ] && [ \"$actual\" = \"$VELNOR_LEDGER_CLI_SHA256\" ]; }; ",
    "git rev-parse HEAD | { read -r head; [ -n \"$VELNOR_LEDGER_CLI_SOURCE_SHA\" ] && [ \"$head\" = \"$VELNOR_LEDGER_CLI_SOURCE_SHA\" ]; }; ",
    "chmod +x \"$f\"",
);

const PUBLISH_SCRIPT: &str = concat!(
    "set -eu; ",
    "out=\"$RUNNER_TEMP/velnor/ledger-cli-out\"; ",
    "meta=\"$RUNNER_TEMP/velnor/ledger-cli-meta\"; ",
    "rm -rf \"$out\" \"$meta\"; ",
    "mkdir -p \"$out\" \"$meta\"; ",
    "cp \"$CARGO_TARGET_DIR/debug/velnor-actions\" \"$out/velnor-actions\"; ",
    "if command -v sha256sum >/dev/null 2>&1; then ",
    "sha256sum \"$out/velnor-actions\" > \"$meta/sha256\"; ",
    "else ",
    "shasum -a 256 \"$out/velnor-actions\" > \"$meta/sha256\"; ",
    "fi; ",
    "read -r digest _ < \"$meta/sha256\"; ",
    "git rev-parse HEAD > \"$meta/head\"; ",
    "read -r head < \"$meta/head\"; ",
    "printf 'sha256=%s\\nsource_sha=%s\\n' \"$digest\" \"$head\" >> \"$GITHUB_OUTPUT\"",
);

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
        "--bin",
        BINARY_NAME,
    ]
    .map(str::to_owned)
    .to_vec();
    let mut env = env.clone();
    env.insert(TARGET_ENV.to_owned(), LEDGER_CLI_TARGET_DIR.to_owned());
    env.insert(LEDGER_CLI_AUTO_INSTALL_ENV.to_owned(), "0".to_owned());
    let mut step = steps::shell_step(LEDGER_CLI_BUILD_STEP, argv, env)?;
    step.role = Some(StepRole::LedgerCliBuild);
    Ok(step)
}

pub fn ledger_cli_publish_step() -> Result<Step, RenderError> {
    sh_step(
        LEDGER_CLI_PUBLISH_STEP,
        PUBLISH_SCRIPT,
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
                LEDGER_CLI_RETENTION_DAYS.to_owned(),
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
        VERIFY_SCRIPT,
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
