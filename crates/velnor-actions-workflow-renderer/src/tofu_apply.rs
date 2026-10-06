//! Protected post-merge `OpenTofu` apply workflow.
//!
//! This is a separate generated workflow: pull-request CI never receives
//! backend credentials, provider tokens, or OIDC permission. Its closed
//! inputs render only one protected default-branch apply path.

use velnor_actions_contract::{TOFU_APPLY_WORKFLOW_PATH, TofuApplyConfig};

use crate::{
    MiseSetup, RenderError, guard, marker, render::RenderedFile, steps,
    tofu_apply_document::tofu_apply_document, yaml::render_yaml,
};

/// Generated workflow display name.
pub const TOFU_APPLY_WORKFLOW_NAME: &str = "OpenTofu Apply";
/// Stable protected apply concurrency group.
pub const TOFU_APPLY_CONCURRENCY_GROUP: &str = "velnor-tofu-apply-${{ github.repository }}";
/// Job ID for the one privileged apply job.
pub const TOFU_APPLY_JOB_ID: &str = "tofu-apply";
/// Workflow timeout bounds provider and backend operations.
pub const TOFU_APPLY_TIMEOUT_MINUTES: i64 = 60;
/// Stable step ID consumed by the `OpenTofu` credential env maps.
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
    /// Exact `OpenTofu` version from the compiled tool catalog.
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

/// Render a distinct protected-main-push `OpenTofu` apply workflow.
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

#[cfg(test)]
pub(super) use crate::tofu_apply_policy::PLAN_REVIEW_JQ;

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
