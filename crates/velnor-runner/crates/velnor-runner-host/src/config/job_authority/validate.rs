mod closure;
mod rules;
use rules::{
    encoded_policy_len, is_git_sha, is_sha256, pr_rule_is_subset, source_policy_allows_ref,
    unique_environments, unique_sorted, valid_concurrency, valid_environment, valid_event_ref,
    valid_identifier, valid_job_key, valid_permission, valid_workflow_path,
};

use std::collections::HashSet;

use super::{
    ApprovedEventContext, ApprovedJobAuthorityEntry, ApprovedJobAuthorityPolicy, ApprovedWorkflow,
    JobAuthorityRole, MAX_CALLS, MAX_CONTEXTS, MAX_ENVIRONMENTS, MAX_JOBS, MAX_NEEDS,
    MAX_POLICY_BYTES, MAX_REFS, MAX_SECRETS, MAX_WORKFLOWS, POLICY_SCHEMA, PermissionLevel,
    WorkflowEvent,
};
use crate::config::RepositoryConnection;
use crate::error::HostError;

pub(crate) fn validate_policies(
    policies: &[ApprovedJobAuthorityPolicy],
    connections: &[RepositoryConnection],
) -> Result<(), HostError> {
    if policies.len() > super::MAX_POLICIES {
        return Err(HostError::Config);
    }
    let mut ids = HashSet::new();
    for policy in policies {
        if !ids.insert(policy.connection_id().clone()) {
            return Err(HostError::Config);
        }
        let connection = connections
            .iter()
            .find(|connection| connection.connection_id == *policy.connection_id())
            .ok_or(HostError::Config)?;
        validate_policy(policy, connection)?;
    }
    Ok(())
}

pub(super) fn validate_policy(
    policy: &ApprovedJobAuthorityPolicy,
    connection: &RepositoryConnection,
) -> Result<(), HostError> {
    if policy.schema_version != POLICY_SCHEMA
        || policy.generation == 0
        || policy.connection_id != connection.connection_id
        || policy.binding_sha256 != super::sha256(&connection.canonical_identity_bytes()?)
        || !is_sha256(&policy.binding_sha256)
        || !is_sha256(&policy.generator.artifact_sha256)
        || !is_git_sha(&policy.generator.source_sha)
        || policy.workflows.is_empty()
        || policy.workflows.len() > MAX_WORKFLOWS
        || encoded_policy_len(policy)? > MAX_POLICY_BYTES
    {
        return Err(HostError::Config);
    }
    let expected =
        super::sha256(&serde_json::to_vec(&policy.content()).map_err(|_| HostError::Config)?);
    if policy.policy_sha256 != expected {
        return Err(HostError::Config);
    }
    validate_workflows(policy, connection)
}

fn validate_workflows(
    policy: &ApprovedJobAuthorityPolicy,
    connection: &RepositoryConnection,
) -> Result<(), HostError> {
    let mut workflow_ids = HashSet::new();
    let mut jobs = 0_usize;
    for workflow in &policy.workflows {
        if workflow.repository_id != connection.repository_id
            || !valid_workflow_path(&workflow.path)
            || !is_sha256(&workflow.content_sha256)
            || workflow.calls.len() > MAX_CALLS
            || workflow.calls.windows(2).any(|pair| pair[0] >= pair[1])
            || !workflow_ids.insert((workflow.repository_id, workflow.path.as_str()))
        {
            return Err(HostError::Config);
        }
        for call in &workflow.calls {
            if call.repository_id != connection.repository_id
                || !valid_workflow_path(&call.path)
                || !is_sha256(&call.content_sha256)
            {
                return Err(HostError::Config);
            }
        }
        jobs = jobs
            .checked_add(workflow.jobs.len())
            .ok_or(HostError::Config)?;
        if jobs > MAX_JOBS {
            return Err(HostError::Config);
        }
        for job in &workflow.jobs {
            validate_job(job, workflow, connection)?;
        }
        if workflow.jobs.windows(2).any(|pair| {
            (&pair[0].root_path, &pair[0].workflow_path, &pair[0].job_key)
                >= (&pair[1].root_path, &pair[1].workflow_path, &pair[1].job_key)
        }) {
            return Err(HostError::Config);
        }
    }
    if policy.workflows.windows(2).any(|pair| {
        (pair[0].repository_id, &pair[0].path) >= (pair[1].repository_id, &pair[1].path)
    }) {
        return Err(HostError::Config);
    }
    closure::validate_closure(policy)?;
    closure::validate_acyclic_closure(policy)
}

fn validate_job(
    job: &ApprovedJobAuthorityEntry,
    workflow: &ApprovedWorkflow,
    connection: &RepositoryConnection,
) -> Result<(), HostError> {
    if job.workflow_path != workflow.path
        || !valid_workflow_path(&job.root_path)
        || !valid_job_key(&job.job_key)
        || !job.permissions_declared
        || job.contexts.is_empty()
        || job.contexts.len() > MAX_CONTEXTS
        || job.contexts.windows(2).any(|pair| pair[0] >= pair[1])
        || job
            .contexts
            .iter()
            .any(|context| !valid_context(context, connection))
        || job.secrets.len() > MAX_SECRETS
        || job.environments.len() > MAX_ENVIRONMENTS
        || job.needs.len() > MAX_NEEDS
        || !unique_sorted(&job.secrets)
        || !unique_sorted(&job.environments)
        || !unique_environments(&job.environments)
        || !unique_sorted(&job.needs)
        || job.secrets.iter().any(|value| !valid_identifier(value))
        || job
            .environments
            .iter()
            .any(|value| !valid_environment(value))
        || job.needs.iter().any(|value| !valid_job_key(value))
        || job.permissions.keys().any(|key| !valid_permission(key))
    {
        return Err(HostError::Config);
    }
    validate_role(job, connection)
}

fn validate_role(
    job: &ApprovedJobAuthorityEntry,
    connection: &RepositoryConnection,
) -> Result<(), HostError> {
    match job.role {
        JobAuthorityRole::Verification => {
            if !job.secrets.is_empty()
                || !job.environments.is_empty()
                || job
                    .permissions
                    .values()
                    .any(|permission| *permission == PermissionLevel::Write)
                || job
                    .condition_sha256
                    .as_ref()
                    .is_some_and(|value| !is_sha256(value))
                || job
                    .concurrency
                    .as_ref()
                    .is_some_and(|value| !valid_concurrency(value))
            {
                return Err(HostError::Config);
            }
        }
        JobAuthorityRole::TrustedPublication => {
            if job
                .condition_sha256
                .as_ref()
                .is_none_or(|value| !is_sha256(value))
                || job
                    .concurrency
                    .as_ref()
                    .is_none_or(|value| !valid_concurrency(value))
                || (job
                    .contexts
                    .iter()
                    .any(|context| context.event == WorkflowEvent::PullRequest)
                    && (!job.secrets.is_empty()
                        || !job.environments.is_empty()
                        || job
                            .permissions
                            .values()
                            .any(|permission| *permission == PermissionLevel::Write)
                        || job
                            .contexts
                            .iter()
                            .filter(|context| context.event == WorkflowEvent::PullRequest)
                            .any(|context| {
                                context.pull_request_rules.iter().any(|rule| {
                                    rule.number.is_none()
                                        || rule.head_repository_id != connection.repository_id
                                })
                            })))
            {
                return Err(HostError::Config);
            }
            if job.contexts.iter().any(|context| {
                context.event == WorkflowEvent::MergeGroup
                    && (!job.secrets.is_empty()
                        || !job.environments.is_empty()
                        || job
                            .permissions
                            .values()
                            .any(|permission| *permission == PermissionLevel::Write))
            }) {
                return Err(HostError::Config);
            }
        }
    }
    Ok(())
}

fn valid_context(context: &ApprovedEventContext, connection: &RepositoryConnection) -> bool {
    if !connection.source_policy.events.contains(&context.event) {
        return false;
    }
    if context.event == WorkflowEvent::PullRequest {
        return context.refs.is_empty()
            && !context.pull_request_rules.is_empty()
            && context.pull_request_rules.len() <= MAX_REFS
            && context
                .pull_request_rules
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            && context.pull_request_rules.iter().all(|rule| {
                rule.base_repository_id == connection.repository_id
                    && rule.head_repository_id > 0
                    && rule.number.is_none_or(|number| number > 0)
                    && valid_event_ref(
                        WorkflowEvent::Push,
                        &format!("refs/heads/{}", rule.base_branch),
                    )
                    && valid_event_ref(
                        WorkflowEvent::Push,
                        &format!("refs/heads/{}", rule.head_branch),
                    )
                    && (connection.source_policy.mode
                        == crate::config::SourcePolicyMode::Repository
                        || connection
                            .source_policy
                            .pull_request_rules
                            .iter()
                            .any(|source| pr_rule_is_subset(rule, source)))
            });
    }
    if !context.pull_request_rules.is_empty()
        || context.refs.is_empty()
        || context.refs.len() > MAX_REFS
        || !unique_sorted(&context.refs)
    {
        return false;
    }
    context.refs.iter().all(|event_ref| {
        valid_event_ref(context.event, event_ref)
            && source_policy_allows_ref(&connection.source_policy, context.event, event_ref)
    })
}
