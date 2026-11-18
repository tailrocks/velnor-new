// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{ReleaseTarget, config::is_hosted_catalog};

use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;
use crate::{RenderError, Schema2WorkflowRequest};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RunnerLane {
    Hosted,
    ScaleSet,
}

pub(super) struct RunnerSpec {
    pub(super) runs_on: Yaml,
    lane: RunnerLane,
}

impl RunnerSpec {
    pub(super) fn hosted(label: &str) -> Result<Self, RenderError> {
        if !is_hosted_catalog(label) {
            return Err(RenderError::InvalidWorkflow(format!(
                "schema2_hosted_runner_not_catalog:{label}"
            )));
        }
        Ok(Self {
            runs_on: Yaml::str(label),
            lane: RunnerLane::Hosted,
        })
    }

    pub(super) fn hosted_release_target(
        label: &str,
        target: ReleaseTarget,
    ) -> Result<Self, RenderError> {
        let allowed_pair = matches!(
            (label, target),
            ("ubuntu-26.04", ReleaseTarget::LinuxX86_64)
                | ("macos-15-intel", ReleaseTarget::MacosX86_64)
        );
        if !allowed_pair || ReleaseTarget::for_runner_label(label) != Some(target) {
            return Err(RenderError::InvalidWorkflow(format!(
                "schema2_hosted_runner_target_mismatch:{label}:{}",
                target.triple()
            )));
        }
        Ok(Self {
            runs_on: Yaml::str(label),
            lane: RunnerLane::Hosted,
        })
    }

    pub(super) fn scale_set(runs_on: Yaml) -> Self {
        Self {
            runs_on,
            lane: RunnerLane::ScaleSet,
        }
    }

    /// A control job placed on the scale set runs only for trusted events, so a
    /// fork pull request never reaches the self-hosted runner. Schema-2 workflows
    /// also admit `workflow_dispatch`, which only a repository writer can start.
    pub(super) fn fence(&self, condition: Option<&str>) -> Option<String> {
        match self.lane {
            RunnerLane::ScaleSet => Some(
                velnor_actions_contract::workflow::fork_fenced_condition_admitting_dispatch(
                    condition,
                ),
            ),
            RunnerLane::Hosted => condition.map(str::to_owned),
        }
    }

    pub(super) fn push_default_shell(&self, fields: &mut Vec<(String, Yaml)>, has_container: bool) {
        if has_container {
            fields.push(crate::runs_on::run_shell_defaults_field(
                crate::runs_on::CONTAINER_RUN_SHELL,
            ));
        } else if self.lane == RunnerLane::ScaleSet {
            fields.push(crate::runs_on::run_shell_defaults_field(
                crate::runs_on::SCALE_SET_RUN_SHELL,
            ));
        }
    }
}

impl Schema2WorkflowRequest {
    /// The runner for a control job: the scale set when `execution.overrides`
    /// places it there, the hosted label otherwise.
    pub(super) fn control_runner(&self, job_id: &str) -> Result<RunnerSpec, RenderError> {
        match self.scale_set_runner(job_id)? {
            Some(runner) => Ok(runner),
            None => RunnerSpec::hosted(&self.hosted_label),
        }
    }

    /// The scale-set runner of a job `execution.overrides` places there, or
    /// `None` when the job keeps the runner its workflow hard-codes.
    pub(super) fn scale_set_runner(&self, job_id: &str) -> Result<Option<RunnerSpec>, RenderError> {
        if !self.scale_set_jobs.contains(job_id) {
            return Ok(None);
        }
        Ok(Some(RunnerSpec::scale_set(runs_on_yaml(
            &self.scale_set.token(),
        )?)))
    }

    /// The runner label the cache steps of a control job are rendered for.
    pub(super) fn control_label(&self, job_id: &str) -> String {
        if self.scale_set_jobs.contains(job_id) {
            self.scale_set.token()
        } else {
            self.hosted_label.clone()
        }
    }
}
