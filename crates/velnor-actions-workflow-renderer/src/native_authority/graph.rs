// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, invalid};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{
    Concurrency, ExecutionMode, Job, JobTimeout, Permissions, Trigger, ValidatorKind,
    VelnorSupportWorkflow, WorkflowIr, WorkflowPolicy,
};

#[derive(Debug, Clone)]
pub(crate) struct AuthorityBinding {
    policy: WorkflowPolicy,
    variants: Vec<(Vec<u8>, Option<velnor_actions_contract::ScaleSetSelector>)>,
    config: velnor_actions_contract::VelnorConfig,
    runner: String,
    tool_pins: Vec<String>,
    validators: Vec<ValidatorKind>,
    commands: Vec<crate::render::ValidatorCommand>,
    workflow_owners: BTreeMap<String, Vec<String>>,
    generated: BTreeMap<String, super::inputs::generated::GeneratedOwner>,
    workflow_required: BTreeSet<String>,
    workflow_sources: BTreeMap<String, Option<String>>,
}

type WorkflowOwners = BTreeMap<String, Vec<String>>;

type Variants = Vec<(Option<Job>, Option<Job>)>;

pub(super) fn build(
    input: &NativeAuthorityInput<'_>,
    authority: &BTreeMap<String, Job>,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(AuthorityBinding, Variants), NativeAuthorityError> {
    let (ir, owners) = metadata_ir(input, authority)?;
    let mut variants = Vec::new();
    let mut projections = vec![(
        projection(&ir.jobs, input.config.workflow.policy)?,
        selector(input.config, None)?,
    )];
    for dispatch in [
        None,
        Some(ExecutionMode::Hosted),
        Some(ExecutionMode::ScaleSet),
        Some(ExecutionMode::Both),
    ] {
        let expanded = match velnor_actions_contract::expand_workflow(&ir, input.config, dispatch) {
            Ok(expanded) => expanded,
            Err(error) if dispatch.is_none() => return Err(error.into()),
            Err(_) => continue,
        };
        projections.push((
            projection(&expanded.jobs, input.config.workflow.policy)?,
            selector(input.config, dispatch)?,
        ));
        variants.push((
            expanded.jobs.get(crate::render::FINAL_JOB_ID).cloned(),
            if input.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1 {
                expanded.jobs.get(crate::render::PLAN_JOB_ID).cloned()
            } else {
                None
            },
        ));
    }
    let generated = super::inputs::generated::inventory(input)?;
    let workflow_required = owners
        .iter()
        .filter(|(owner, _)| !generated.contains_key(*owner))
        .flat_map(|(_, ids)| ids.iter().cloned())
        .chain(
            generated
                .values()
                .flat_map(|owner| owner.required.iter().cloned()),
        )
        .collect();
    Ok((
        AuthorityBinding {
            workflow_sources: super::task_sources::admit(input.config, input.workflow_sources)?,
            generated,
            workflow_required,
            commands: super::support_commands::build(input)?,
            config: input.config.clone(),
            runner: input.runner.to_owned(),
            tool_pins: super::tools::catalog_tool_pins(catalog),
            policy: input.config.workflow.policy,
            variants: projections,
            validators: validators(input)?,
            workflow_owners: owners,
        },
        variants,
    ))
}

fn metadata_ir(
    input: &NativeAuthorityInput<'_>,
    authority: &BTreeMap<String, Job>,
) -> Result<(WorkflowIr, WorkflowOwners), NativeAuthorityError> {
    let mut jobs = authority.clone();
    let mut owners = BTreeMap::<String, Vec<String>>::new();
    for fact in input.logical_jobs {
        let mut job = metadata(input.runner);
        job.display_name.clone_from(&fact.id);
        job.runs_on.clone_from(&fact.runs_on);
        job.check_runner.clone_from(&fact.check_runner);
        job.needs.clone_from(&fact.needs);
        if let LogicalJobSource::WorkflowTask { owner }
        | LogicalJobSource::GeneratedWorkflowTask { owner, .. } = &fact.source
        {
            owners
                .entry(owner.clone())
                .or_default()
                .push(fact.id.clone());
        }
        jobs.insert(fact.id.clone(), job);
    }
    if !jobs.contains_key(crate::render::PLAN_JOB_ID) {
        let mut plan = metadata(input.runner);
        plan.steps.push(crate::steps::write_request_step(
            crate::steps::PLAN_OPERATION,
        )?);
        jobs.insert(crate::render::PLAN_JOB_ID.to_owned(), plan);
    }
    jobs.insert(
        crate::native_job_builders::LINT_JOB_ID.to_owned(),
        metadata(input.runner),
    );
    Ok((
        WorkflowIr {
            name: input.config.workflow.name.clone(),
            triggers: Trigger {
                pull_request_types: Vec::new(),
                push_branches: Vec::new(),
                merge_group: false,
                workflow_dispatch: None,
                schedule: None,
            },
            permissions: Permissions::default(),
            concurrency: Concurrency {
                group: String::new(),
                cancel_in_progress: String::new(),
            },
            jobs,
            cache_mode: None,
        },
        owners,
    ))
}

impl AuthorityBinding {
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
                .filter(|id| task.owns_projected_job_id(id, context.bun_lane_plan.as_ref()))
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
        super::task_policies::matches(
            &self.config,
            &self.generated,
            &self.workflow_sources,
            policy,
        )
    }
}

fn projection(
    jobs: &BTreeMap<String, Job>,
    policy: WorkflowPolicy,
) -> Result<Vec<u8>, NativeAuthorityError> {
    let projected = jobs
        .iter()
        .filter(|(id, _)| {
            policy == WorkflowPolicy::VelnorRepositoryV1
                || id.as_str() != crate::render::PLAN_JOB_ID
        })
        .map(|(id, job)| {
            let mut metadata = job.clone();
            metadata.steps.clear();
            if matches!(
                id.as_str(),
                crate::render::PLAN_JOB_ID | crate::render::FINAL_JOB_ID
            ) {
                metadata.runs_on.clear();
                metadata.check_runner = None;
                metadata.needs.clear();
            }
            {
                metadata.display_name.clear();
                metadata.services.clear();
                metadata.timeout_minutes = JobTimeout::DEFAULT;
                metadata.condition = None;
                metadata.permissions = None;
                metadata.environment = None;
                metadata.cache_mode = None;
            }
            (id.clone(), metadata)
        })
        .collect::<BTreeMap<_, _>>();
    velnor_actions_contract::canonical_json_bytes(&projected).map_err(Into::into)
}

fn validators(
    input: &NativeAuthorityInput<'_>,
) -> Result<Vec<ValidatorKind>, NativeAuthorityError> {
    let mut configured = Vec::new();
    for kind in ValidatorKind::consumer_verify() {
        if input
            .config
            .workflow
            .verify
            .jobs
            .iter()
            .any(|job| job == kind.job_id())
        {
            configured.push(kind);
        }
    }
    if configured.len() != input.config.workflow.verify.jobs.len() {
        return Err(invalid("unknown_verify_job"));
    }
    let mut validators = if input.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1 {
        input.config.workflow.policy.support_workflow().validators
    } else {
        Vec::new()
    };
    for kind in configured {
        if !validators.contains(&kind) {
            validators.push(kind);
        }
    }
    if input.cargo_workspaces.is_empty() {
        validators.retain(|kind| *kind != ValidatorKind::CargoDeny);
    }
    Ok(validators)
}

fn metadata(runner: &str) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Plan".to_owned(),
        runs_on: runner.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
        cache_mode: None,
    }
}

fn selector(
    config: &velnor_actions_contract::VelnorConfig,
    dispatch: Option<ExecutionMode>,
) -> Result<Option<velnor_actions_contract::ScaleSetSelector>, NativeAuthorityError> {
    let Some(execution) = &config.execution else {
        return Ok(None);
    };
    let mut execution = execution.clone();
    if let Some(mode) = dispatch {
        execution.mode = Some(mode);
    }
    if execution.emits_scale_set_selector() {
        Ok(Some(execution.scale_selector()?))
    } else {
        Ok(None)
    }
}
