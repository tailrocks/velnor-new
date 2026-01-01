//! Typed step builders for the protected `OpenTofu` apply workflow.

use std::collections::BTreeMap;

use velnor_actions_contract::TofuApplyConfig;

use crate::tofu_apply::TofuApplySpec;
use crate::tofu_apply::{AWS_CREDENTIALS_STEP_ID, AWS_CREDENTIALS_USES, AWS_CREDENTIALS_VERSION};
use crate::tofu_apply_command::{
    aws_env, plan_paths_env, tofu_exec, tofu_logged_step, tofu_shell_step,
};
use crate::tofu_apply_policy::{BACKEND_REVIEW_JQ, PLAN_REVIEW_JQ};
use crate::{RenderError, commands, steps, steps_plain::plain_step_to_yaml, yaml::Yaml};

pub(super) fn checkout_yaml(uses: &str) -> Result<Yaml, RenderError> {
    let step = steps::checkout_step(uses)?;
    plain_step_to_yaml(&step)
}

pub(super) fn required_tokens_step(config: &TofuApplyConfig) -> Result<Yaml, RenderError> {
    let checks = config
        .github_tokens
        .iter()
        .map(|token| format!("test -n \"${{{}}}\"", token.secret_name))
        .collect::<Vec<_>>()
        .join(" && ");
    let script = format!(
        "set -euo pipefail; {checks} || {{ printf '%s\\n' 'required GitHub provider secret is missing' >&2; exit 1; }}"
    );
    let env = config
        .github_tokens
        .iter()
        .map(|token| {
            (
                token.secret_name.clone(),
                format!("${{{{ secrets.{} }}}}", token.secret_name),
            )
        })
        .collect();
    tofu_shell_step(
        "Check required GitHub provider secrets",
        vec!["bash".to_owned(), "-c".to_owned(), script],
        &env,
    )
}

pub(super) fn aws_credentials_step(config: &TofuApplyConfig) -> Result<Yaml, RenderError> {
    let with = BTreeMap::from([
        ("aws-region".to_owned(), config.backend.region.clone()),
        ("mask-aws-account-id".to_owned(), "true".to_owned()),
        ("output-credentials".to_owned(), "true".to_owned()),
        ("output-env-credentials".to_owned(), "false".to_owned()),
        (
            "role-session-name".to_owned(),
            "velnor-tofu-${{ github.run_id }}".to_owned(),
        ),
        ("role-to-assume".to_owned(), config.role_arn.clone()),
    ]);
    for (key, value) in &with {
        crate::expressions::check_with_value(key, value)?;
    }
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Assume AWS role with GitHub OIDC"),
        ),
        ("id".to_owned(), Yaml::str(AWS_CREDENTIALS_STEP_ID)),
        (
            "uses".to_owned(),
            Yaml::annotated(AWS_CREDENTIALS_USES, AWS_CREDENTIALS_VERSION),
        ),
        (
            "with".to_owned(),
            Yaml::Map(
                with.into_iter()
                    .map(|(key, value)| (key, Yaml::str(value)))
                    .collect(),
            ),
        ),
    ]))
}

pub(super) fn install_opentofu_step(version: &str) -> Result<Yaml, RenderError> {
    tofu_shell_step(
        "Install pinned OpenTofu",
        vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            "--yes".to_owned(),
            format!("opentofu@{version}"),
        ],
        &BTreeMap::new(),
    )
}

pub(super) fn tofu_init_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    let config = &spec.config;
    tofu_logged_step(
        "Initialize locked S3 state backend",
        {
            let mut command = tofu_exec(spec);
            command.extend([
                "init".to_owned(),
                "-input=false".to_owned(),
                "-lockfile=readonly".to_owned(),
                "-no-color".to_owned(),
                format!("-backend-config=bucket={}", config.backend.bucket),
                format!("-backend-config=key={}", config.backend.key),
                format!("-backend-config=region={}", config.backend.region),
                "-backend-config=encrypt=true".to_owned(),
                "-backend-config=use_lockfile=true".to_owned(),
            ]);
            command
        },
        aws_env(config, false),
        "test \"$tofu_status\" -eq 0",
    )
}

pub(super) fn tofu_backend_validation_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    let script = format!(
        "set -euo pipefail; jq -e {} .terraform/terraform.tfstate >/dev/null 2>&1",
        commands::quote_run_arg(&jq_contract(BACKEND_REVIEW_JQ))?
    );
    let env = BTreeMap::from([
        (
            "VELNOR_BACKEND_BUCKET".to_owned(),
            spec.config.backend.bucket.clone(),
        ),
        (
            "VELNOR_BACKEND_KEY".to_owned(),
            spec.config.backend.key.clone(),
        ),
        (
            "VELNOR_BACKEND_REGION".to_owned(),
            spec.config.backend.region.clone(),
        ),
    ]);
    tofu_shell_step(
        "Require configured S3 remote backend",
        vec!["bash".to_owned(), "-c".to_owned(), script],
        &env,
    )
}

pub(super) fn branch_head_guard_step(name: &str, branch: &str) -> Result<Yaml, RenderError> {
    let ref_name = format!("refs/heads/{branch}");
    let script = format!(
        "set -euo pipefail; guard_dir=\"${{{{ runner.temp }}}}/velnor-tofu-guard\"; mkdir -p -- \"$guard_dir\"; git rev-parse --verify HEAD >\"$guard_dir/checked-out\"; printf '%s\\n' \"$GITHUB_SHA\" >\"$guard_dir/requested\"; git ls-remote --exit-code --refs origin '{ref_name}' | cut -f1 >\"$guard_dir/remote\"; cmp -s -- \"$guard_dir/checked-out\" \"$guard_dir/requested\" && cmp -s -- \"$guard_dir/checked-out\" \"$guard_dir/remote\""
    );
    tofu_shell_step(
        name,
        vec!["bash".to_owned(), "-c".to_owned(), script],
        &BTreeMap::new(),
    )
}

pub(super) fn tofu_plan_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    let mut argv = tofu_exec(spec);
    argv.extend([
        "plan".to_owned(),
        "-input=false".to_owned(),
        "-lock=true".to_owned(),
        "-refresh=true".to_owned(),
        "-detailed-exitcode".to_owned(),
        "-no-color".to_owned(),
    ]);
    argv.push("-out=$VELNOR_TOFU_PLAN_FILE".to_owned());
    tofu_logged_step(
        "Create full refresh plan",
        argv,
        aws_env(&spec.config, true),
        "case \"$tofu_status\" in 0|2) test -s \"$VELNOR_TOFU_PLAN_FILE\" ;; *) exit 1 ;; esac",
    )
}

pub(super) fn tofu_plan_review_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    let mut command = tofu_exec(spec);
    command.extend([
        "show".to_owned(),
        "-json".to_owned(),
        "$VELNOR_TOFU_PLAN_FILE".to_owned(),
    ]);
    let script = format!(
        "set -euo pipefail; {} 2>/dev/null | jq -e {} >/dev/null 2>&1",
        commands::join_argv_for_run(&command)?,
        commands::quote_run_arg(&jq_contract(PLAN_REVIEW_JQ))?
    );
    tofu_shell_step(
        "Fail closed on saved plan actions",
        vec!["bash".to_owned(), "-c".to_owned(), script],
        &plan_paths_env(),
    )
}

pub(super) fn tofu_apply_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    tofu_logged_step(
        "Apply reviewed saved plan",
        {
            let mut command = tofu_exec(spec);
            command.extend([
                "apply".to_owned(),
                "-input=false".to_owned(),
                "-lock=true".to_owned(),
                "-no-color".to_owned(),
                "$VELNOR_TOFU_PLAN_FILE".to_owned(),
            ]);
            command
        },
        aws_env(&spec.config, true),
        "test \"$tofu_status\" -eq 0",
    )
}

pub(super) fn tofu_live_verify_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    tofu_logged_step(
        "Verify desired configuration converged",
        {
            let mut command = tofu_exec(spec);
            command.extend([
                "plan".to_owned(),
                "-input=false".to_owned(),
                "-refresh-only".to_owned(),
                "-lock=true".to_owned(),
                "-refresh=true".to_owned(),
                "-detailed-exitcode".to_owned(),
                "-no-color".to_owned(),
            ]);
            command
        },
        aws_env(&spec.config, true),
        "test \"$tofu_status\" -eq 0",
    )
}

pub(super) fn tofu_cleanup_step() -> Result<Yaml, RenderError> {
    let mut step = tofu_shell_step(
        "Remove private saved plan",
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            "rm -f -- \"$VELNOR_TOFU_PLAN_FILE\" \"$VELNOR_TOFU_LOG_FILE\"".to_owned(),
        ],
        &plan_paths_env(),
    )?;
    if let Yaml::Map(entries) = &mut step {
        entries.insert(1, ("if".to_owned(), Yaml::str("always()")));
    }
    Ok(step)
}

/// Collapse documented jq whitespace so fixed shell scripts stay single-line.
fn jq_contract(filter: &str) -> String {
    filter.split_whitespace().collect::<Vec<_>>().join(" ")
}
