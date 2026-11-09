// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{AuthorityBinding, projection};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{Job, ValidatorKind, VelnorSupportWorkflow, WorkflowPolicy};

impl AuthorityBinding {
    pub(crate) fn check_mise_setup(
        &self,
        id: &str,
        target: &str,
    ) -> Result<Option<crate::MiseSetup>, crate::RenderError> {
        self.check_mise.for_job(id, target)
    }

    pub(crate) fn validate(
        &self,
        jobs: &BTreeMap<String, Job>,
        policy: WorkflowPolicy,
        support: Option<&VelnorSupportWorkflow>,
        context: &crate::render::RenderContext,
    ) -> Result<Vec<usize>, crate::RenderError> {
        let fail = || crate::RenderError::InvalidWorkflow("native_authority_graph".to_owned());
        if policy != self.policy
            || support.map_or(&[][..], |support| support.validators.as_slice()) != self.validators
        {
            return Err(fail());
        }
        let actual = projection(jobs, policy).map_err(|_| fail())?;
        if context.generator_version != env!("CARGO_PKG_VERSION")
            || context.runs_on != self.runner
            || context.tool_pins != self.tool_pins
            || context.pull_request_cache_policy != self.config.workflow.pull_request_cache_policy
            || context.mise_quiet != self.config.workflow.mise_quiet
        {
            return Err(fail());
        }
        let variants = self
            .variants
            .iter()
            .enumerate()
            .filter_map(|(index, (graph, selector))| {
                (*graph == actual && *selector == context.scale_set_selector).then_some(index)
            })
            .collect::<Vec<_>>();
        if variants.is_empty() {
            return Err(fail());
        }
        if context.validator_commands != self.commands {
            return Err(fail());
        }
        let commands = context
            .validator_commands
            .iter()
            .map(|command| command.validator)
            .collect::<BTreeSet<_>>();
        let expected = self
            .validators
            .iter()
            .copied()
            .filter(|validator| *validator != ValidatorKind::Alint)
            .collect::<BTreeSet<_>>();
        if commands != expected || commands.len() != context.validator_commands.len() {
            return Err(fail());
        }
        if context.workflow_tasks.len() != self.workflow_owners.len() {
            return Err(fail());
        }
        let mut seen = BTreeSet::new();
        for task in &context.workflow_tasks {
            if !self.matches_task_config(task) {
                return Err(fail());
            }
            if !seen.insert(task.id()) {
                return Err(fail());
            }
            let Some(expected) = self.workflow_owners.get(task.id()) else {
                return Err(fail());
            };
            let actual = jobs
                .keys()
                .filter(|id| task.owns_job_id(id))
                .map(|id| {
                    id.strip_suffix(velnor_actions_contract::HOSTED_SUFFIX)
                        .or_else(|| id.strip_suffix(velnor_actions_contract::SCALE_SUFFIX))
                        .unwrap_or(id)
                        .to_owned()
                })
                .collect::<BTreeSet<_>>();
            if actual != expected.iter().cloned().collect() {
                return Err(fail());
            }
        }
        Ok(variants)
    }
    pub(crate) fn required_workflow_ids(&self, jobs: &BTreeMap<String, Job>) -> Vec<String> {
        jobs.keys()
            .filter(|id| {
                let base = id
                    .strip_suffix(velnor_actions_contract::HOSTED_SUFFIX)
                    .or_else(|| id.strip_suffix(velnor_actions_contract::SCALE_SUFFIX))
                    .unwrap_or(id);
                self.workflow_required.contains(base)
            })
            .cloned()
            .collect()
    }

    fn matches_task_config(&self, policy: &crate::verification_jobs::WorkflowTaskPolicy) -> bool {
        super::super::task_policies::matches(
            &self.config,
            &self.generated,
            &self.workflow_sources,
            self.runner_family_tools.as_deref(),
            policy,
        )
    }

    pub(crate) fn qualify_direct(
        jobs: &BTreeMap<String, Job>,
        policy: WorkflowPolicy,
        config: &velnor_actions_contract::VelnorConfig,
        runner: &str,
        tool_pins: Vec<String>,
    ) -> Result<Self, crate::RenderError> {
        let graph = projection(jobs, policy).map_err(|error| {
            crate::RenderError::InvalidWorkflow(format!("native_authority_graph:{error:?}"))
        })?;
        Ok(Self {
            check_mise: super::super::check_mise::CheckMiseSetups::default(),
            policy,
            variants: vec![(graph, None)],
            config: config.clone(),
            runner: runner.to_owned(),
            tool_pins,
            validators: Vec::new(),
            commands: Vec::new(),
            workflow_owners: BTreeMap::new(),
            generated: BTreeMap::new(),
            workflow_required: BTreeSet::new(),
            workflow_sources: BTreeMap::new(),
            runner_family_tools: None,
        })
    }
}
