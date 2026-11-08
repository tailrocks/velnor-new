// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, invalid};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{
    Concurrency, ExecutionMode, Job, JobTimeout, Permissions, Trigger, ValidatorKind, WorkflowIr,
    WorkflowPolicy,
};

#[derive(Debug, Clone)]
pub(crate) struct AuthorityBinding {
    check_mise: super::check_mise::CheckMiseSetups,
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

mod binding;
mod task_bindings;

pub(super) fn build(
    input: &NativeAuthorityInput<'_>,
    authority: &BTreeMap<String, Job>,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(AuthorityBinding, Variants), NativeAuthorityError> {
    let (ir, owners) = metadata_ir(input, authority)?;
    let task_bindings = task_bindings::rust(input)?;
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
        let expansion = match velnor_actions_contract::expand_workflow_with_task_bindings(
            &ir,
            input.config,
            dispatch,
            &task_bindings,
        ) {
            Ok(expanded) => expanded,
            Err(error) if dispatch.is_none() => return Err(error.into()),
            Err(_) => continue,
        };
        projections.push((
            projection(&expansion.ir.jobs, input.config.workflow.policy)?,
            selector(input.config, dispatch)?,
        ));
        variants.push((
            expansion.ir.jobs.get(crate::render::FINAL_JOB_ID).cloned(),
            if input.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1 {
                expansion.ir.jobs.get(crate::render::PLAN_JOB_ID).cloned()
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
            check_mise: super::check_mise::CheckMiseSetups::build(input)?,
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
        job.outputs.clone_from(&fact.outputs);
        if matches!(&fact.source, LogicalJobSource::WorkflowTask { owner } if owner == "runner-image-family")
            && fact.id == velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_JOB_ID
            && input.config.runner_image_family.is_some()
        {
            job.env = velnor_actions_contract::JobEnv::RustToolchain;
        }
        if let LogicalJobSource::NamedCheck { id } = &fact.source {
            job.steps.push(named_check_metadata(input, id)?);
        }
        if matches!(fact.source, LogicalJobSource::StateProducer { .. }) {
            job.steps
                .push(crate::state_seal::producer_steps::state_produced_step()?);
        }
        if matches!(fact.source, LogicalJobSource::LedgerCli) {
            job.steps.push(crate::ledger_cli_build_step(
                &velnor_actions_mise::ToolCatalog::pinned(),
                &BTreeMap::new(),
            )?);
        }
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
        },
        owners,
    ))
}

fn named_check_metadata(
    input: &NativeAuthorityInput<'_>,
    id: &str,
) -> Result<velnor_actions_contract::Step, NativeAuthorityError> {
    if input.config.checks.iter().any(|check| check.id == id) {
        let task_id =
            velnor_actions_contract::task_id_for_stack("mise", id, "check", "default", None)?;
        let mut step = crate::steps::internal_step("Execute named check", "execute-check-v1")?;
        if let velnor_actions_contract::StepKind::Internal { env, .. } = &mut step.kind {
            env.insert("VELNOR_TASK_ID".to_owned(), task_id);
        }
        return Ok(step);
    }
    let check = input
        .config
        .native_checks
        .iter()
        .find(|check| check.id == id)
        .ok_or_else(|| invalid("native_check_job_identity"))?;
    let task_id = velnor_actions_contract::task_id_for_internal("native-check", id)?;
    let version = input
        .published_generator_lock
        .map_or(env!("CARGO_PKG_VERSION"), |lock| {
            lock.source_preview
                .as_ref()
                .map_or(lock.generator.version.as_str(), |preview| {
                    preview.receipt.version.as_str()
                })
        });
    Ok(velnor_actions_contract::Step {
        name: "Execute native Homebrew check".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: velnor_actions_contract::StepKind::NativeCheckExecution {
            check_id: id.to_owned(),
            operation: check.operation,
            task_id,
            sdk_version: version.to_owned(),
        },
    })
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
                metadata.cache_access = velnor_actions_contract::CacheAccess::Read;
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
        outputs: BTreeMap::new(),
        display_name: "Plan".to_owned(),
        runs_on: runner.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        env: velnor_actions_contract::JobEnv::Unset,
        steps: Vec::new(),
        cache_access: velnor_actions_contract::CacheAccess::Read,
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
