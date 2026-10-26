// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::{TOFU_APPLY_WORKFLOW_PATH, TofuApplyConfig};

use crate::{
    MiseSetup, RenderError, guard, marker, render::RenderedFile, steps,
    tofu_apply_document::tofu_apply_document,
};

pub const TOFU_APPLY_WORKFLOW_NAME: &str = "OpenTofu Apply";
pub const TOFU_APPLY_CONCURRENCY_GROUP: &str = "velnor-tofu-apply-${{ github.repository }}";
pub const TOFU_APPLY_JOB_ID: &str = "tofu-apply";
pub const TOFU_APPLY_TIMEOUT_MINUTES: u16 = JobTimeout::DEFAULT.minutes();
pub const AWS_CREDENTIALS_STEP_ID: &str = "aws-credentials";
pub const AWS_CREDENTIALS_USES: &str =
    "aws-actions/configure-aws-credentials@e1253824e5c10ff9df46874f81ed3ec929e19cfd";
pub const AWS_CREDENTIALS_VERSION: &str = "v6.3.0";

#[derive(Debug, Clone)]
pub struct TofuApplySpec {
    pub config: TofuApplyConfig,
    pub default_branch: String,
    pub runs_on: String,
    pub checkout_uses: String,
    pub mise_setup: MiseSetup,
    pub opentofu_version: String,
    pub generator_version: String,
}

impl TofuApplySpec {
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
        if !crate::commands::is_exact_or_project_version(&self.opentofu_version) {
            return Err(RenderError::BadCommand(
                "bad_tofu_apply_opentofu_version".to_owned(),
            ));
        }
        steps::validate_uses(AWS_CREDENTIALS_USES)?;
        Ok(())
    }
}

pub fn render_tofu_apply_workflow(spec: &TofuApplySpec) -> Result<RenderedFile, RenderError> {
    spec.validate()?;
    let document = tofu_apply_document(spec)?;
    let text = crate::render::fallback::render_checked_workflow(
        TOFU_APPLY_WORKFLOW_PATH,
        &document,
        &spec.generator_version,
    )?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(RenderedFile {
        path: TOFU_APPLY_WORKFLOW_PATH.to_owned(),
        bytes: text,
    })
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
