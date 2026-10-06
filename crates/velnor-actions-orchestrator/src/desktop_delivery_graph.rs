//! Desktop topology staged before complete operation-plan qualification.
//!
//! Staged until complete source/recipe registry and protected signing authority
//! are admitted. No raw argv, scripts, tool selectors, or native runtime policy.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Concurrency, ContractError, Job, JobTimeout, Permissions, Step, StepKind, Trigger, WorkflowIr,
    config::DesktopDeliveryConfig,
    workflow::{
        dispatch::{DispatchInput, DispatchInputType, WorkflowDispatch},
        jobs::ScheduleTrigger,
        permissions::PermissionLevel,
    },
};

pub(super) struct DesktopGraphSteps {
    pub(super) unsigned: Vec<Step>,
}

pub(super) fn release(
    config: &DesktopDeliveryConfig,
    default_branch: &str,
    steps: DesktopGraphSteps,
) -> Result<WorkflowIr, ContractError> {
    validate_policy(config, default_branch)?;
    if config.sign_tags {
        return Err(failure("desktop_signing_authority_unqualified"));
    }
    let trigger = Trigger {
        pull_request_types: Vec::new(),
        push_tags: Vec::new(),
        push_branches: Vec::new(),
        merge_group: false,
        workflow_dispatch: Some(WorkflowDispatch {
            inputs: vec![DispatchInput {
                name: "mode".to_owned(),
                input_type: DispatchInputType::Choice,
                required: true,
                description: Some("Unsigned validation drill".to_owned()),
                options: vec!["validate".to_owned()],
                default: Some("validate".to_owned()),
            }],
        }),
        schedule: None,
    };
    let job = job(
        "Unsigned validation",
        format!(
            "${{{{ github.repository == '{}' && github.event_name == 'workflow_dispatch' && inputs.mode == 'validate' }}}}",
            config.repository
        ),
        steps.unsigned,
    )?;
    workflow(
        "Desktop release",
        trigger,
        "velnor-desktop-release-${{ github.ref }}",
        false,
        "build",
        job,
    )
}

pub(super) fn cadence(
    config: &DesktopDeliveryConfig,
    default_branch: &str,
    scheduled: bool,
    steps: Vec<Step>,
) -> Result<WorkflowIr, ContractError> {
    validate_policy(config, default_branch)?;
    let event = if scheduled {
        "(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')"
    } else {
        "(github.event_name == 'push' || github.event_name == 'workflow_dispatch')"
    };
    let kind = if scheduled { "scheduled" } else { "merge" };
    let trigger = Trigger {
        pull_request_types: Vec::new(),
        push_tags: Vec::new(),
        push_branches: if scheduled {
            Vec::new()
        } else {
            vec![default_branch.to_owned()]
        },
        merge_group: false,
        workflow_dispatch: Some(WorkflowDispatch { inputs: Vec::new() }),
        schedule: if scheduled {
            Some(ScheduleTrigger {
                cron: vec!["41 4 * * 1".to_owned()],
            })
        } else {
            None
        },
    };
    let job = job(
        &format!("Desktop {kind} cadence"),
        format!(
            "${{{{ github.repository == '{}' && github.ref == 'refs/heads/{default_branch}' && github.ref == format('refs/heads/{{0}}', github.event.repository.default_branch) && {event} }}}}",
            config.repository
        ),
        steps,
    )?;
    let mut graph = workflow(
        &format!("Desktop {kind} cadence"),
        trigger,
        &format!("desktop-{kind}-${{{{ github.repository }}}}-${{{{ github.ref }}}}"),
        true,
        &format!("desktop-{kind}"),
        job,
    )?;
    graph.run_name = Some(format!(
        "Desktop {kind} cadence · ${{{{ github.event_name }}}}"
    ));
    graph.validate()?;
    Ok(graph)
}

fn validate_policy(config: &DesktopDeliveryConfig, branch: &str) -> Result<(), ContractError> {
    config.validate(".velnor/config.toml")?;
    if !config.enabled {
        return Err(failure("desktop_graph_disabled"));
    }
    if !velnor_actions_contract::is_valid_branch_name(branch) {
        return Err(failure("desktop_default_branch_invalid"));
    }
    Ok(())
}

fn job(name: &str, condition: String, steps: Vec<Step>) -> Result<Job, ContractError> {
    let exact_checkout_first = steps
        .first()
        .is_some_and(|step| matches!(step.kind, StepKind::Action { .. }) && allowed_step(step));
    if !exact_checkout_first
        || steps
            .iter()
            .skip(1)
            .any(|step| !matches!(step.kind, StepKind::SourceBoundHelper { .. }))
    {
        return Err(failure("desktop_requires_owner_operations"));
    }
    Ok(Job {
        cache_mode: None,
        display_name: name.to_owned(),
        runs_on: "macos-26".to_owned(),
        timeout_minutes: JobTimeout::new(90)?,
        needs: Vec::new(),
        condition: Some(condition),
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps,
    })
}

fn allowed_step(step: &Step) -> bool {
    match &step.kind {
        StepKind::SourceBoundHelper { .. } => true,
        StepKind::Action { uses, with, env } => {
            uses == "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
                && with
                    == &BTreeMap::from([
                        ("persist-credentials".to_owned(), "false".to_owned()),
                        ("fetch-depth".to_owned(), "0".to_owned()),
                        ("ref".to_owned(), "${{ github.sha }}".to_owned()),
                    ])
                && env.is_empty()
                && step.condition.is_none()
        }
        StepKind::Shell { .. } | StepKind::Internal { .. } => false,
    }
}

fn workflow(
    name: &str,
    triggers: Trigger,
    group: &str,
    cancel: bool,
    id: &str,
    job: Job,
) -> Result<WorkflowIr, ContractError> {
    let graph = WorkflowIr {
        cache_mode: velnor_actions_contract::CacheMode::Read,
        run_name: None,
        name: name.to_owned(),
        triggers,
        permissions: Permissions {
            actions: PermissionLevel::None,
            ..Permissions::default()
        },
        concurrency: Concurrency {
            group: group.to_owned(),
            cancel_in_progress: format!("${{{{ {cancel} }}}}"),
        },
        jobs: BTreeMap::from([(id.to_owned(), job)]),
    };
    graph.validate()?;
    Ok(graph)
}

fn failure(problem: &str) -> ContractError {
    ContractError::identity("desktop_graph", problem)
}

#[cfg(test)]
#[path = "desktop_delivery_graph_tests.rs"]
mod tests;
