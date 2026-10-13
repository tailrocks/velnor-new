//! Protected post-merge OpenTofu apply workflow.
//!
//! This is a separate generated workflow: pull-request CI never receives
//! backend credentials, provider tokens, or OIDC permission. Its closed
//! inputs render only one protected default-branch apply path.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, TOFU_APPLY_WORKFLOW_PATH, TofuApplyConfig};

use crate::{
    MiseSetup, RenderError, commands, guard, marker,
    render::RenderedFile,
    setup::mise_setup_step,
    steps,
    steps_plain::plain_step_to_yaml,
    yaml::{Yaml, render_yaml},
};

/// Generated workflow display name.
pub const TOFU_APPLY_WORKFLOW_NAME: &str = "OpenTofu Apply";
/// Stable protected apply concurrency group.
pub const TOFU_APPLY_CONCURRENCY_GROUP: &str = "velnor-tofu-apply-${{ github.repository }}";
/// Job ID for the one privileged apply job.
pub const TOFU_APPLY_JOB_ID: &str = "tofu-apply";
/// Workflow timeout bounds provider and backend operations.
pub const TOFU_APPLY_TIMEOUT_MINUTES: i64 = 60;
/// Stable step ID consumed by the OpenTofu credential env maps.
pub const AWS_CREDENTIALS_STEP_ID: &str = "aws-credentials";
/// Locally mirrored immutable AWS credentials action pin; actionlint owns its inventory.
pub const AWS_CREDENTIALS_USES: &str =
    "aws-actions/configure-aws-credentials@e1253824e5c10ff9df46874f81ed3ec929e19cfd";
/// Version comment paired with [`AWS_CREDENTIALS_USES`].
pub const AWS_CREDENTIALS_VERSION: &str = "v6.3.0";

/// All caller-controlled values needed by the workflow renderer.
#[derive(Debug, Clone)]
pub struct TofuApplySpec {
    /// Closed per-repository apply configuration.
    pub config: TofuApplyConfig,
    /// Protected default branch; used as the workflow's only trigger branch.
    pub default_branch: String,
    /// Literal versioned Ubuntu runner label.
    pub runs_on: String,
    /// Pinned checkout action.
    pub checkout_uses: String,
    /// Pinned Mise action and executable digest.
    pub mise_setup: MiseSetup,
    /// Exact OpenTofu version from the compiled tool catalog.
    pub opentofu_version: String,
    /// Exact generator marker version.
    pub generator_version: String,
}

impl TofuApplySpec {
    /// Validate caller-supplied scalars and action pins.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for invalid renderer inputs.
    pub fn validate(&self) -> Result<(), RenderError> {
        self.config
            .validate(TOFU_APPLY_WORKFLOW_PATH)
            .map_err(RenderError::Contract)?;
        if !is_valid_default_branch(&self.default_branch) {
            return Err(RenderError::InvalidWorkflow(
                "bad_tofu_apply_default_branch".to_owned(),
            ));
        }
        guard::validate_runs_on(&self.runs_on)?;
        marker::validate_version(&self.generator_version)?;
        self.mise_setup.validate()?;
        steps::validate_uses(&self.checkout_uses)?;
        if !self.checkout_uses.starts_with("actions/checkout@") {
            return Err(RenderError::BadActionRef(format!(
                "not_checkout:{}",
                self.checkout_uses
            )));
        }
        if !is_exact_version(&self.opentofu_version) {
            return Err(RenderError::BadCommand(
                "bad_tofu_apply_opentofu_version".to_owned(),
            ));
        }
        steps::validate_uses(AWS_CREDENTIALS_USES)?;
        Ok(())
    }
}

/// Render a distinct protected-main-push OpenTofu apply workflow.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid typed inputs or workflow size.
pub fn render_tofu_apply_workflow(spec: &TofuApplySpec) -> Result<RenderedFile, RenderError> {
    spec.validate()?;
    let document = tofu_apply_document(spec)?;
    let text = marker::with_marker(&spec.generator_version, &render_yaml(&document))?;
    crate::workflow_size::check_workflow_size(TOFU_APPLY_WORKFLOW_PATH, &text)?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(RenderedFile {
        path: TOFU_APPLY_WORKFLOW_PATH.to_owned(),
        bytes: text,
    })
}

/// Build the single job with credentials scoped only to the steps that need them.
fn tofu_apply_document(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    let config = &spec.config;
    let checkout = checkout_yaml(&spec.checkout_uses)?;
    let mise = plain_step_to_yaml(&mise_setup_step(&spec.mise_setup)?)?;
    let check_tokens = required_tokens_step(config)?;
    let aws = aws_credentials_step(config)?;
    let install = install_opentofu_step(&spec.opentofu_version)?;
    let init = tofu_init_step(spec)?;
    let plan = tofu_plan_step(spec)?;
    let review = tofu_plan_review_step(spec)?;
    let apply = tofu_apply_step(spec)?;
    let verify = tofu_live_verify_step(spec)?;
    let cleanup = tofu_cleanup_step()?;

    let steps = vec![
        checkout,
        mise,
        check_tokens,
        aws,
        install,
        init,
        plan,
        review,
        apply,
        verify,
        cleanup,
    ];
    let job = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(TOFU_APPLY_WORKFLOW_NAME)),
        (
            "if".to_owned(),
            Yaml::str("${{ github.ref_protected == true }}"),
        ),
        ("runs-on".to_owned(), Yaml::str(spec.runs_on.clone())),
        (
            "environment".to_owned(),
            Yaml::str(config.environment.clone()),
        ),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(TOFU_APPLY_TIMEOUT_MINUTES),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("contents".to_owned(), Yaml::str("read")),
                ("id-token".to_owned(), Yaml::str("write")),
            ]),
        ),
        (
            "defaults".to_owned(),
            Yaml::Map(vec![(
                "run".to_owned(),
                Yaml::Map(vec![
                    (
                        "shell".to_owned(),
                        Yaml::str("bash --noprofile --norc -euo pipefail {0}"),
                    ),
                    (
                        "working-directory".to_owned(),
                        Yaml::str(config.root.as_str().to_owned()),
                    ),
                ]),
            )]),
        ),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ]);
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(TOFU_APPLY_WORKFLOW_NAME)),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "push".to_owned(),
                Yaml::Map(vec![(
                    "branches".to_owned(),
                    Yaml::Seq(vec![Yaml::str(spec.default_branch.clone())]),
                )]),
            )]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("none"))]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(TOFU_APPLY_CONCURRENCY_GROUP)),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(TOFU_APPLY_JOB_ID.to_owned(), job)]),
        ),
    ]))
}

fn checkout_yaml(uses: &str) -> Result<Yaml, RenderError> {
    let step = steps::checkout_step(uses)?;
    plain_step_to_yaml(&step)
}

fn required_tokens_step(config: &TofuApplyConfig) -> Result<Yaml, RenderError> {
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
        env,
    )
}

fn aws_credentials_step(config: &TofuApplyConfig) -> Result<Yaml, RenderError> {
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

fn install_opentofu_step(version: &str) -> Result<Yaml, RenderError> {
    tofu_shell_step(
        "Install pinned OpenTofu",
        vec![
            "mise".to_owned(),
            "install".to_owned(),
            "--yes".to_owned(),
            format!("opentofu@{version}"),
        ],
        BTreeMap::new(),
    )
}

fn tofu_init_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
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
        "test \"$tofu_status\" -eq 0".to_owned(),
    )
}

fn tofu_plan_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
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
        "case \"$tofu_status\" in 0|2) test -s \"$VELNOR_TOFU_PLAN_FILE\" ;; *) exit 1 ;; esac"
            .to_owned(),
    )
}

fn tofu_plan_review_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    let mut command = tofu_exec(spec);
    command.extend([
        "show".to_owned(),
        "-json".to_owned(),
        "$VELNOR_TOFU_PLAN_FILE".to_owned(),
    ]);
    let script = format!(
        "set -euo pipefail; {} 2>/dev/null | jq -e {} >/dev/null 2>&1",
        commands::join_argv_for_run(&command)?,
        commands::quote_run_arg(PLAN_REVIEW_JQ)
    );
    tofu_shell_step(
        "Fail closed on saved plan actions",
        vec!["bash".to_owned(), "-c".to_owned(), script],
        plan_paths_env(),
    )
}

fn tofu_apply_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
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
        "test \"$tofu_status\" -eq 0".to_owned(),
    )
}

fn tofu_live_verify_step(spec: &TofuApplySpec) -> Result<Yaml, RenderError> {
    tofu_logged_step(
        "Verify live state is current",
        {
            let mut command = tofu_exec(spec);
            command.extend([
                "plan".to_owned(),
                "-refresh-only".to_owned(),
                "-input=false".to_owned(),
                "-lock=true".to_owned(),
                "-refresh=true".to_owned(),
                "-detailed-exitcode".to_owned(),
                "-no-color".to_owned(),
            ]);
            command
        },
        aws_env(&spec.config, true),
        "test \"$tofu_status\" -eq 0".to_owned(),
    )
}

fn tofu_cleanup_step() -> Result<Yaml, RenderError> {
    let mut step = tofu_shell_step(
        "Remove private saved plan",
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            "rm -f -- \"$VELNOR_TOFU_PLAN_FILE\" \"$VELNOR_TOFU_LOG_FILE\"".to_owned(),
        ],
        plan_paths_env(),
    )?;
    if let Yaml::Map(entries) = &mut step {
        entries.insert(1, ("if".to_owned(), Yaml::str("always()")));
    }
    Ok(step)
}

/// Build a fixed OpenTofu command with output redirected away from logs.
fn tofu_logged_step(
    name: &str,
    command: Vec<String>,
    mut env: BTreeMap<String, String>,
    result_check: String,
) -> Result<Yaml, RenderError> {
    let script = format!(
        "set -euo pipefail; umask 077; if {} > \"$VELNOR_TOFU_LOG_FILE\" 2>&1; then tofu_status=0; else tofu_status=$?; fi; {}",
        commands::join_argv_for_run(&command)?,
        result_check
    );
    env.extend(plan_paths_env());
    tofu_shell_step(name, vec!["bash".to_owned(), "-c".to_owned(), script], env)
}

/// AWS credential outputs are opt-in per step; provider tokens are separate.
fn aws_env(config: &TofuApplyConfig, include_github_tokens: bool) -> BTreeMap<String, String> {
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

fn plan_paths_env() -> BTreeMap<String, String> {
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
fn tofu_shell_step(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Yaml, RenderError> {
    validate_tofu_env(&env)?;
    commands::validate_command_argv(&argv)?;
    commands::validate_env(&env)?;
    let step_env = crate::toolchain_env::with_credential_scrub(&env);
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

fn tofu_exec(spec: &TofuApplySpec) -> Vec<String> {
    vec![
        "mise".to_owned(),
        "exec".to_owned(),
        format!("opentofu@{}", spec.opentofu_version),
        "--".to_owned(),
        "tofu".to_owned(),
    ]
}

fn is_exact_version(value: &str) -> bool {
    let mut components = value.split('.');
    components
        .by_ref()
        .take(3)
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && components.next().is_none()
        && value.split('.').count() == 3
}

fn is_valid_default_branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("//")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
}

/// Fail-closed saved-plan review: no deletes/replacements, incomplete/error plans,
/// deferred changes, failed checks, or provisioner/action invocations are admitted.
const PLAN_REVIEW_JQ: &str = "type == \"object\" and (.format_version | type == \"string\" and startswith(\"1.\")) and .errored == false and (.resource_changes | type == \"array\") and (.configuration | type == \"object\") and (.planned_values | type == \"object\") and ((.checks // []) | type == \"array\" and all(.[]; .status == \"pass\")) and ((has(\"complete\") | not) or .complete == true) and ((has(\"applyable\") | not) or .applyable == true) and ((has(\"deferred_changes\") | not) or .deferred_changes == []) and ((has(\"action_invocations\") | not) or .action_invocations == []) and ((has(\"removed\") | not) or .removed == []) and all(.resource_changes[]; ((.mode == \"managed\") and (.change.actions == [\"no-op\"] or .change.actions == [\"create\"] or .change.actions == [\"update\"]) or ((.mode == \"data\") and (.change.actions == [\"no-op\"] or .change.actions == [\"read\"]))))";

#[cfg(test)]
mod tests {
    use super::{
        AWS_CREDENTIALS_STEP_ID, PLAN_REVIEW_JQ, TOFU_APPLY_CONCURRENCY_GROUP, TofuApplySpec,
        render_tofu_apply_workflow,
    };
    use crate::MiseSetup;
    use velnor_actions_contract::{
        GitHubTokenSecret, S3BackendConfig, TofuApplyConfig, Utf8RepoRelDir,
    };

    const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
    const MISE: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";

    fn spec() -> TofuApplySpec {
        TofuApplySpec {
            config: TofuApplyConfig {
                root: Utf8RepoRelDir::from_raw("infra".to_owned()),
                environment: "production".to_owned(),
                role_arn: "arn:aws:iam::123456789012:role/velnor-tofu".to_owned(),
                backend: S3BackendConfig {
                    bucket: "example-tofu-state".to_owned(),
                    key: "chainargos/control-plane.tfstate".to_owned(),
                    region: "us-east-1".to_owned(),
                },
                github_tokens: vec![
                    GitHubTokenSecret {
                        organization: "chainargos".to_owned(),
                        secret_name: "GH_TOKEN_CHAINARGOS".to_owned(),
                    },
                    GitHubTokenSecret {
                        organization: "tailrocks".to_owned(),
                        secret_name: "GH_TOKEN_TAILROCKS".to_owned(),
                    },
                ],
            },
            default_branch: "main".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            checkout_uses: CHECKOUT.to_owned(),
            mise_setup: MiseSetup {
                uses: MISE.to_owned(),
                version: "2026.9.18".to_owned(),
                sha256: crate::setup::MISE_BINARY_SHA256_LINUX_X64.to_owned(),
            },
            opentofu_version: "1.13.1".to_owned(),
            generator_version: "0.1.0".to_owned(),
        }
    }

    #[test]
    fn renders_only_protected_main_push_and_scoped_apply_capabilities() {
        let file = render_tofu_apply_workflow(&spec()).expect("render");
        assert_eq!(file.path, ".github/workflows/tofu-apply.yml");
        let yaml = file.bytes;
        assert!(yaml.contains("branches:\n      - main"), "{yaml}");
        assert!(!yaml.contains("pull_request"), "{yaml}");
        assert!(!yaml.contains("workflow_dispatch"), "{yaml}");
        assert!(
            yaml.contains("if: ${{ github.ref_protected == true }}"),
            "{yaml}"
        );
        assert!(yaml.contains("environment: production"), "{yaml}");
        assert!(yaml.contains("cancel-in-progress: false"), "{yaml}");
        assert!(yaml.contains("contents: none"), "{yaml}");
        assert!(yaml.contains("id-token: write"), "{yaml}");
        assert!(yaml.contains("id: aws-credentials"), "{yaml}");
        assert!(yaml.contains(AWS_CREDENTIALS_STEP_ID), "{yaml}");
        assert!(yaml.contains("use_lockfile=true"), "{yaml}");
        assert!(yaml.contains("-refresh=true"), "{yaml}");
        assert!(yaml.contains("tofu show -json"), "{yaml}");
        assert!(yaml.contains("Apply reviewed saved plan"), "{yaml}");
        assert!(yaml.contains("-refresh-only"), "{yaml}");
        assert!(yaml.contains("secrets.GH_TOKEN_CHAINARGOS"), "{yaml}");
        assert!(yaml.contains("secrets.GH_TOKEN_TAILROCKS"), "{yaml}");
        assert!(!yaml.contains("${{ secrets.AWS"), "{yaml}");
        assert!(yaml.contains(TOFU_APPLY_CONCURRENCY_GROUP), "{yaml}");
    }

    #[test]
    fn output_matches_generator_golden_snapshot() {
        let file = render_tofu_apply_workflow(&spec()).expect("render");
        assert_eq!(
            file.bytes,
            include_str!("../tests/goldens/tofu-apply.yml"),
            "byte-exact generated workflow golden"
        );
    }

    #[test]
    fn plan_review_rejects_destructive_or_incomplete_plans() {
        for expected in [
            ".errored == false",
            ".complete == true",
            ".applyable == true",
            ".deferred_changes == []",
            ".action_invocations == []",
            "[\"create\"]",
            "[\"update\"]",
            "[\"no-op\"]",
            "[\"read\"]",
        ] {
            assert!(PLAN_REVIEW_JQ.contains(expected), "missing {expected}");
        }
        assert!(!PLAN_REVIEW_JQ.contains("[\"delete\"]"));
    }

    #[test]
    fn invalid_branch_and_floating_tofu_versions_fail_closed() {
        let mut config = spec();
        config.default_branch = "main && echo unsafe".to_owned();
        assert!(config.validate().is_err());
        let mut config = spec();
        config.opentofu_version = "latest".to_owned();
        assert!(config.validate().is_err());
    }
}
