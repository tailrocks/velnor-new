// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{BTreeMap, RenderError, commands, guard, marker, steps, validator_tools};
use velnor_actions_contract::{
    CI_WORKFLOW_PATH, PLAN_JOB_ID as CONTRACT_PLAN_JOB_ID, PullRequestCachePolicy,
    REQUIRED_CONDITION as CONTRACT_REQUIRED_CONDITION, REQUIRED_JOB_ID as CONTRACT_REQUIRED_JOB_ID,
    REQUIRED_VERDICT_DISPLAY_NAME as CONTRACT_REQUIRED_VERDICT_DISPLAY_NAME, ScaleSetSelector,
    ValidatorKind,
};

pub use crate::matrix::{
    MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV, MatrixSource,
    NOT_AFFECTED_JOBS_OUTPUT, PLAN_ATTEMPT_ENV, PLAN_ATTEMPT_OUTPUT, PLAN_ID_OUTPUT, PLAN_STEP_ID,
    RUN_KEY_OUTPUT,
};
pub use crate::plan_gate::not_affected_gate;
pub use crate::setup::MiseSetup;

pub const WORKFLOW_PATH: &str = CI_WORKFLOW_PATH;
pub const JACTIONLINT_PATH: &str = velnor_actions_actionlint::JACTIONLINT_CONFIG_PATH;
pub const EXPECTED_PR_TYPES: &[&str] = &[
    "opened",
    "edited",
    "synchronize",
    "reopened",
    "ready_for_review",
];
pub const CONCURRENCY_GROUP: &str =
    "velnor-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}";
pub const CONCURRENCY_CANCEL: &str = "${{ github.event_name == 'pull_request' }}";
pub const FINAL_JOB_ID: &str = CONTRACT_REQUIRED_JOB_ID;
pub const FINAL_DISPLAY_NAME: &str = CONTRACT_REQUIRED_VERDICT_DISPLAY_NAME;
pub const FINAL_CONDITION: &str = CONTRACT_REQUIRED_CONDITION;

pub const UPLOAD_STEP_CONDITION: &str = "always()";
pub const PLAN_JOB_ID: &str = CONTRACT_PLAN_JOB_ID;
pub const TASK_JOB_ID: &str = "velnor-task";
pub const CANDIDATE_JOB_ID: &str = "candidate";
pub const ALINT_USES: &str = "asamarts/alint@265d9fdc720bb8f59d00b87e33e4149869f77327";

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub authority_steps: super::CanonicalAuthoritySteps,
    pub generator_version: String,
    pub report_helper_version: String,
    pub runs_on: String,
    pub scale_set_selector: Option<ScaleSetSelector>,
    pub staged_binary: String,
    pub request_dir: String,
    pub checkout_uses: String,
    pub validator_commands: Vec<ValidatorCommand>,
    pub preseed: bool,
    pub workflow_tasks: Vec<crate::verification_jobs::WorkflowTaskPolicy>,
    pub pull_request_cache_policy: PullRequestCachePolicy,
    pub mise_quiet: bool,
    pub plan_consumer_env: BTreeMap<String, String>,
    pub tool_pins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorCommand {
    pub validator: ValidatorKind,
    pub name: String,
    pub argv: Vec<String>,
    pub prepare_argv: Vec<String>,
}

pub use crate::lane_share::RenderedWorkflow;
pub use crate::tree::{RenderedFile, RenderedSymlink, RenderedTree};
pub use velnor_actions_contract::{AGENTS_MD_PATH, CLAUDE_MD_PATH};

impl RenderContext {
    pub fn validate(&self) -> Result<(), RenderError> {
        self.validate_runtime_authority()?;
        marker::validate_version(&self.generator_version)?;
        marker::validate_version(&self.report_helper_version)?;
        guard::validate_runs_on(&self.runs_on)?;
        guard::validate_staged_binary(&self.staged_binary, &self.report_helper_version)?;
        guard::validate_request_dir(&self.request_dir)?;
        steps::checkout_step(&self.checkout_uses).map(|_| ())?;
        for command in &self.validator_commands {
            if command.validator == ValidatorKind::Jactionlint {
                return Err(RenderError::BadCommand(
                    "jactionlint_not_support".to_owned(),
                ));
            }
            if command.name.trim().is_empty() {
                return Err(RenderError::BadCommand("empty_validator_name".to_owned()));
            }
            validator_tools::validate_validator_tool_closure(command)?;
            if !command.prepare_argv.is_empty() {
                commands::validate_command_argv(&command.prepare_argv)?;
            }
            commands::validate_command_argv(&command.argv)?;
        }
        Ok(())
    }

    pub(crate) fn validate_runtime_authority(&self) -> Result<(), RenderError> {
        if self.authority_steps.matches_runtime(self) {
            Ok(())
        } else {
            Err(RenderError::InvalidWorkflow(
                "trusted_plan_runtime_context".to_owned(),
            ))
        }
    }
}
