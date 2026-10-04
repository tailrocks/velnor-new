//! Optional protected observer outside the exact Required verification graph.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, Job, JobTimeout, PermissionLevel, Permissions,
    SourceBoundHelper, SourceBoundOperation, ToolCacheDomain, VelnorConfig,
};
use velnor_actions_mise::catalog::{delivery_tools, qualification::DistributionHost};
use velnor_actions_workflow_renderer::{
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
    verification_observer::{observer_setup_steps, observer_step, simulation_step},
};

use crate::{OrchestratorError, delivery_emit::delivery_tool_context::delivery_tool_context};

/// Fixed observer identity; it is never a Required dependency.
pub(crate) const OBSERVER_JOB_ID: &str = "verification-observer";

/// Build the source-owned observer record and its complete helper registry.
///
/// The bootstrap and tool-preparation records are returned with the observer
/// record because the renderer admits every source-bound step against the
/// enclosing workflow registry.
/// # Errors
/// Rejects unsupported hosts, missing owned SDK recipes, or invalid bindings.
fn observer_records(
    tools: &velnor_actions_workflow_renderer::delivery_tools::DeliveryToolContext,
    repository: &str,
    branch: &str,
    label: &str,
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let host = observer_host(label)?;
    let recipe = delivery_tools::execution_recipe(host, NativeCredentialScope::GithubIssueWrite)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    delivery_tools::validate_recipe(host, NativeCredentialScope::GithubIssueWrite, &recipe)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    let source = velnor_actions_workflow_renderer::verification_observer::observer_source(version)?;
    let operation = SourceBoundOperation::VerificationObserver;
    let descriptor = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
    )
    .map_err(|error| OrchestratorError::Contract {
        problem: error.to_string(),
    })?;
    let invocation =
        HelperInvocation::compiled(descriptor, Vec::new(), Vec::new()).map_err(|error| {
            OrchestratorError::Contract {
                problem: error.to_string(),
            }
        })?;
    let environment = BTreeMap::from([
        ("APPROVED_REPOSITORY".to_owned(), repository.to_owned()),
        (
            "GITHUB_REPOSITORY".to_owned(),
            "${{ github.repository }}".to_owned(),
        ),
        ("DEFAULT_BRANCH".to_owned(), branch.to_owned()),
        ("REF".to_owned(), "${{ github.ref }}".to_owned()),
        (
            "REF_PROTECTED".to_owned(),
            "${{ github.ref_protected }}".to_owned(),
        ),
        (
            "EVENT_NAME".to_owned(),
            "${{ github.event_name }}".to_owned(),
        ),
        ("SOURCE_SHA".to_owned(), "${{ github.sha }}".to_owned()),
        ("RUN_ID".to_owned(), "${{ github.run_id }}".to_owned()),
        (
            "RUN_ATTEMPT".to_owned(),
            "${{ github.run_attempt }}".to_owned(),
        ),
        (
            "REQUIRED_RESULT".to_owned(),
            "${{ needs.required.result }}".to_owned(),
        ),
    ]);
    let observer = CompiledSourceHelper::compiled(invocation, source)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    let bootstrap = tools
        .mise
        .bootstrap(ToolCacheDomain::Full, label)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?
        .helper
        .clone();
    Ok(vec![bootstrap, tools.preparation.clone(), observer])
}

fn observer_host(label: &str) -> Result<DistributionHost, OrchestratorError> {
    match label {
        "ubuntu-24.04" | "ubuntu-26.04" => Ok(DistributionHost::LinuxAmd64),
        "ubuntu-24.04-arm" | "ubuntu-26.04-arm" => Ok(DistributionHost::LinuxArm64),
        "macos-26" | "macos-15" => Ok(DistributionHost::MacosArm64),
        _ => Err(OrchestratorError::Contract {
            problem: format!("verification_observer_unsupported_host:{label}"),
        }),
    }
}

/// Attach opt-in observer and the explicit Required failure simulation.
/// # Errors
/// Rejects missing Required jobs, malformed identities, or unpinned tools.
pub(crate) fn attach_observer(
    jobs: &mut BTreeMap<String, Job>,
    config: &VelnorConfig,
    repository: &str,
    branch: &str,
    label: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    if !jobs.contains_key(FINAL_JOB_ID) {
        return Err(OrchestratorError::Contract {
            problem: "verification_observer_missing_Required".to_owned(),
        });
    }
    let tools = delivery_tool_context(config, label)?;
    let records = observer_records(&tools, repository, branch, label, env!("CARGO_PKG_VERSION"))?;
    if config
        .workflow
        .verification
        .as_ref()
        .is_some_and(|verification| verification.workflow_dispatch)
    {
        let plan = jobs
            .get_mut(PLAN_JOB_ID)
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "verification_observer_missing_Plan".to_owned(),
            })?;
        let at = plan
            .steps
            .iter()
            .position(|step| {
                matches!(&step.kind, velnor_actions_contract::StepKind::Internal { operation }
                if operation == velnor_actions_workflow_renderer::steps::PLAN_OPERATION)
            })
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "verification_observer_missing_plan_operation".to_owned(),
            })?;
        plan.steps.insert(at, simulation_step()?);
    }
    let mut steps = observer_setup_steps(&tools, label)?;
    let observer = records.last().ok_or_else(|| OrchestratorError::Contract {
        problem: "verification_observer_source_missing".to_owned(),
    })?;
    steps.push(observer_step(repository, branch, FINAL_JOB_ID, observer)?);
    jobs.insert(
        OBSERVER_JOB_ID.to_owned(),
        Job {
            cache_mode: None,
            display_name: "Nightly failure observer".to_owned(),
            runs_on: label.to_owned(),
            timeout_minutes: JobTimeout::RELEASE,
            needs: vec![FINAL_JOB_ID.to_owned()],
            condition: Some(
                velnor_actions_contract::workflow::observer::observer_condition(repository, branch),
            ),
            permissions: Some(Permissions {
                contents: PermissionLevel::None,
                pull_requests: PermissionLevel::None,
                id_token: PermissionLevel::None,
                actions: PermissionLevel::None,
                issues: PermissionLevel::Write,
                pages: PermissionLevel::None,
                attestations: PermissionLevel::None,
            }),
            tool_producer: None,
            mbx_producer: None,
            source_producer: None,
            native_pages_deploy: None,
            native_publish: None,
            outputs: Vec::new(),
            environment: Some("verification-alerts".to_owned()),
            steps,
        },
    );
    Ok(records)
}

#[cfg(test)]
#[path = "verification_observer_tests.rs"]
mod verification_observer_tests;
