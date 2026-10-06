//! Safe command and environment helpers for the generated apply workflow.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, TofuApplyConfig};

use crate::tofu_apply::TofuApplySpec;
use crate::{RenderError, commands, steps_plain::plain_step_to_yaml, yaml::Yaml};

/// Build a fixed `OpenTofu` command with output redirected away from logs.
#[expect(
    clippy::needless_pass_by_value,
    reason = "call sites build and yield an owned fixed argv"
)]
pub(super) fn tofu_logged_step(
    name: &str,
    command: Vec<String>,
    mut env: BTreeMap<String, String>,
    result_check: &str,
) -> Result<Yaml, RenderError> {
    let script = format!(
        "set -euo pipefail; umask 077; if {} > \"$VELNOR_TOFU_LOG_FILE\" 2>&1; then tofu_status=0; else tofu_status=$?; fi; {}",
        commands::join_argv_for_run(&command)?,
        result_check
    );
    env.extend(plan_paths_env());
    tofu_shell_step(name, vec!["bash".to_owned(), "-c".to_owned(), script], &env)
}

/// AWS credential outputs are opt-in per step; provider tokens are separate.
pub(super) fn aws_env(
    config: &TofuApplyConfig,
    include_github_tokens: bool,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        (
            "AWS_ACCESS_KEY_ID".to_owned(),
            "${{ steps.aws-credentials.outputs.aws-access-key-id }}".to_owned(),
        ),
        (
            "AWS_DEFAULT_REGION".to_owned(),
            config.backend.region.clone(),
        ),
        ("AWS_REGION".to_owned(), config.backend.region.clone()),
        (
            "AWS_SECRET_ACCESS_KEY".to_owned(),
            "${{ steps.aws-credentials.outputs.aws-secret-access-key }}".to_owned(),
        ),
        (
            "AWS_SESSION_TOKEN".to_owned(),
            "${{ steps.aws-credentials.outputs.aws-session-token }}".to_owned(),
        ),
    ]);
    if include_github_tokens {
        let tokens = config
            .github_tokens
            .iter()
            .map(|token| {
                [
                    "\"",
                    token.organization.as_str(),
                    "\":\"${{ secrets.",
                    token.secret_name.as_str(),
                    " }}\"",
                ]
                .concat()
            })
            .collect::<Vec<_>>()
            .join(",");
        env.insert("TF_VAR_github_tokens".to_owned(), format!("{{{tokens}}}"));
    }
    env
}

pub(super) fn plan_paths_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "VELNOR_TOFU_LOG_FILE".to_owned(),
            "${{ runner.temp }}/velnor-tofu-apply.log".to_owned(),
        ),
        (
            "VELNOR_TOFU_PLAN_FILE".to_owned(),
            "${{ runner.temp }}/velnor-tofu-apply.tfplan".to_owned(),
        ),
    ])
}

/// Create a validated fixed shell step while admitting only workflow-owned env keys.
pub(super) fn tofu_shell_step(
    name: &str,
    argv: Vec<String>,
    env: &BTreeMap<String, String>,
) -> Result<Yaml, RenderError> {
    validate_tofu_env(env)?;
    commands::validate_command_argv(&argv)?;
    commands::validate_env(env)?;
    let step_env = crate::toolchain_env::with_credential_scrub(env);
    let run = if crate::commands::is_inline_shell(&argv) {
        let mut scripted = argv;
        scripted[2] = crate::toolchain_env::with_credential_unset_script(&scripted[2]);
        scripted
    } else {
        crate::toolchain_env::with_env_unset_argv(&argv)
    };
    let step = Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell { run, env: step_env },
    };
    plain_step_to_yaml(&step)
}

fn validate_tofu_env(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for key in env.keys() {
        if key == "TF_VAR_github_tokens"
            || matches!(
                key.as_str(),
                "AWS_ACCESS_KEY_ID"
                    | "AWS_DEFAULT_REGION"
                    | "AWS_REGION"
                    | "AWS_SECRET_ACCESS_KEY"
                    | "AWS_SESSION_TOKEN"
                    | "VELNOR_BACKEND_BUCKET"
                    | "VELNOR_BACKEND_KEY"
                    | "VELNOR_BACKEND_REGION"
                    | "VELNOR_TOFU_LOG_FILE"
                    | "VELNOR_TOFU_PLAN_FILE"
            )
            || (key.starts_with("GH_TOKEN_")
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
        {
            continue;
        }
        return Err(RenderError::BadCommand(format!(
            "tofu_apply_env_denied:{key}"
        )));
    }
    Ok(())
}

pub(super) fn tofu_exec(spec: &TofuApplySpec) -> Vec<String> {
    vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        format!("opentofu@{}", spec.opentofu_version),
        "--".to_owned(),
        "tofu".to_owned(),
    ]
}
