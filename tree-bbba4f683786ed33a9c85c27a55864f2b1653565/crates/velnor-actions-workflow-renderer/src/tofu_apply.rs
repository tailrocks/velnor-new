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
    let reject_stale = branch_head_guard_step(
        "Reject stale default-branch revision",
        &spec.default_branch,
    )?;
    let mise = plain_step_to_yaml(&mise_setup_step(&spec.mise_setup)?)?;
    let check_tokens = required_tokens_step(config)?;
    let aws = aws_credentials_step(config)?;
    let install = install_opentofu_step(&spec.opentofu_version)?;
    let init = tofu_init_step(spec)?;
    let backend = tofu_backend_validation_step(spec)?;
    let before_plan = branch_head_guard_step(
        "Recheck default-branch head before planning",
        &spec.default_branch,
    )?;
    let plan = tofu_plan_step(spec)?;
    let review = tofu_plan_review_step(spec)?;
    let before_apply = branch_head_guard_step(
        "Recheck default-branch head before apply",
        &spec.default_branch,
    )?;
    let apply = tofu_apply_step(spec)?;
    let verify = tofu_live_verify_step(spec)?;
    let cleanup = tofu_cleanup_step()?;

    let steps = vec![
        checkout,
        reject_stale,
        mise,
        check_tokens,
        aws,
        install,
        init,
        backend,
        before_plan,
        plan,
        review,
        before_apply,
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
            "env".to_owned(),
            Yaml::Map(
                crate::toolchain_env::job_level_env()
                    .into_iter()
                    .map(|(key, value)| (key, Yaml::str(value)))
                    .collect(),
            ),
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

use crate::tofu_apply_steps::{
    aws_credentials_step, branch_head_guard_step, checkout_yaml, install_opentofu_step,
    required_tokens_step, tofu_apply_step, tofu_backend_validation_step, tofu_cleanup_step,
    tofu_init_step, tofu_live_verify_step, tofu_plan_review_step, tofu_plan_step,
};

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

#[cfg(test)]
#[path = "tofu_apply_tests.rs"]
mod tests;
