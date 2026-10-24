use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    ApprovedEventContext, ApprovedJobAuthorityEntry, ApprovedJobAuthorityPolicy, ApprovedWorkflow,
    ConcurrencyControl, ConcurrencyScope, GeneratorIdentity, JobAuthorityRole, POLICY_SCHEMA,
    PermissionLevel, WorkflowCall, WorkflowEvent, sha256,
};
use crate::config::RepositoryConnection;
use crate::error::HostError;

const INVENTORY_SCHEMA: u32 = 1;

/// Build one host policy JSON object from generator inventory and reviewed inputs.
///
/// The inventory records workflow facts, not repository origin. The operator
/// must review those facts for this connection before importing the result.
pub(super) fn policy_json_from_inventory(
    connection: &RepositoryConnection,
    inventory_json: &str,
    generator_source_sha: &str,
    generator_artifact_sha256: &str,
    generation: u64,
) -> Result<String, HostError> {
    let inventory: JobAuthorityInventory =
        serde_json::from_str(inventory_json).map_err(|_| HostError::Config)?;
    validate_inventory(&inventory, generator_source_sha)?;

    let mut policy = ApprovedJobAuthorityPolicy {
        schema_version: POLICY_SCHEMA,
        connection_id: connection.connection_id.clone(),
        binding_sha256: sha256(&connection.canonical_identity_bytes()?),
        generation,
        policy_sha256: String::new(),
        generator: GeneratorIdentity {
            source_sha: generator_source_sha.to_owned(),
            artifact_sha256: generator_artifact_sha256.to_owned(),
        },
        workflows: inventory
            .workflows
            .iter()
            .map(|workflow| convert_workflow(workflow, connection))
            .collect::<Result<Vec<_>, _>>()?,
    };
    policy.canonicalize();
    policy.policy_sha256 =
        sha256(&serde_json::to_vec(&policy.content()).map_err(|_| HostError::Config)?);
    policy.validate_for(connection)?;
    serde_json::to_string(&policy).map_err(|_| HostError::Config)
}

fn validate_inventory(
    inventory: &JobAuthorityInventory,
    generator_source_sha: &str,
) -> Result<(), HostError> {
    if inventory.schema_version != INVENTORY_SCHEMA
        || inventory.generator_source_sha != generator_source_sha
        || !valid_hex(generator_source_sha, 40)
        || !valid_hex(&inventory.inventory_sha256, 64)
    {
        return Err(HostError::Config);
    }
    let content = InventoryContent {
        schema_version: inventory.schema_version,
        generator_source_sha: &inventory.generator_source_sha,
        workflows: &inventory.workflows,
    };
    let digest = serde_json::to_vec(&content).map_err(|_| HostError::Config)?;
    if sha256(&digest) != inventory.inventory_sha256 {
        return Err(HostError::Config);
    }
    Ok(())
}

fn convert_workflow(
    workflow: &InventoryWorkflow,
    connection: &RepositoryConnection,
) -> Result<ApprovedWorkflow, HostError> {
    Ok(ApprovedWorkflow {
        repository_id: connection.repository_id,
        path: workflow.path.clone(),
        content_sha256: workflow.content_sha256.clone(),
        calls: workflow
            .calls
            .iter()
            .map(|call| WorkflowCall {
                repository_id: connection.repository_id,
                path: call.path.clone(),
                content_sha256: call.content_sha256.clone(),
            })
            .collect(),
        jobs: workflow
            .jobs
            .iter()
            .map(|job| convert_job(job, connection))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn convert_job(
    job: &InventoryJob,
    connection: &RepositoryConnection,
) -> Result<ApprovedJobAuthorityEntry, HostError> {
    Ok(ApprovedJobAuthorityEntry {
        root_path: job.root_path.clone(),
        workflow_path: job.workflow_path.clone(),
        job_key: job.job_key.clone(),
        role: job.role,
        contexts: convert_contexts(&job.contexts, connection)?,
        permissions_declared: job.permissions_declared,
        permissions: convert_permissions(&job.permissions)?,
        secrets: job.secrets.clone(),
        environments: job.environments.clone(),
        condition_sha256: job.condition_sha256.clone(),
        concurrency: job
            .concurrency
            .as_ref()
            .map(convert_concurrency)
            .transpose()?,
        needs: job.needs.clone(),
    })
}

fn convert_permissions(
    permissions: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, PermissionLevel>, HostError> {
    permissions
        .iter()
        .map(|(name, level)| {
            let level = match level.as_str() {
                "none" => PermissionLevel::None,
                "read" => PermissionLevel::Read,
                "write" => PermissionLevel::Write,
                _ => return Err(HostError::Config),
            };
            Ok((name.clone(), level))
        })
        .collect()
}

fn convert_concurrency(value: &InventoryConcurrency) -> Result<ConcurrencyControl, HostError> {
    Ok(ConcurrencyControl {
        scope: match value.scope.as_str() {
            "workflow" => ConcurrencyScope::Workflow,
            "job" => ConcurrencyScope::Job,
            _ => return Err(HostError::Config),
        },
        group: value.group.clone(),
        cancel_in_progress: value.cancel_in_progress.clone(),
    })
}

fn convert_contexts(
    contexts: &[InventoryContext],
    connection: &RepositoryConnection,
) -> Result<Vec<ApprovedEventContext>, HostError> {
    contexts
        .iter()
        .map(|context| convert_context(context, connection))
        .collect()
}

fn convert_context(
    context: &InventoryContext,
    connection: &RepositoryConnection,
) -> Result<ApprovedEventContext, HostError> {
    let event = parse_event(&context.event)?;
    if !connection.source_policy.events.contains(&event) {
        return Err(HostError::Config);
    }
    if event == WorkflowEvent::PullRequest {
        if !context.branch_filters.is_empty() || !context.tag_filters.is_empty() {
            return Err(HostError::Config);
        }
        let pull_request_rules = connection
            .source_policy
            .pull_request_rules
            .iter()
            .filter(|rule| {
                context.pull_request_base_branches.is_empty()
                    || context
                        .pull_request_base_branches
                        .contains(&rule.base_branch)
            })
            .cloned()
            .collect::<Vec<_>>();
        if pull_request_rules.is_empty() {
            return Err(HostError::Config);
        }
        return Ok(ApprovedEventContext {
            event,
            refs: Vec::new(),
            pull_request_rules,
        });
    }

    if !context.pull_request_base_branches.is_empty() {
        return Err(HostError::Config);
    }
    let refs = context_refs(context, event, connection)?;
    if refs.is_empty() {
        return Err(HostError::Config);
    }
    Ok(ApprovedEventContext {
        event,
        refs,
        pull_request_rules: Vec::new(),
    })
}

fn context_refs(
    context: &InventoryContext,
    event: WorkflowEvent,
    connection: &RepositoryConnection,
) -> Result<Vec<String>, HostError> {
    let mut refs = Vec::new();
    let branch_event = match event {
        WorkflowEvent::Push => context.tag_filters.is_empty(),
        WorkflowEvent::WorkflowDispatch | WorkflowEvent::MergeGroup | WorkflowEvent::Schedule => {
            true
        }
        WorkflowEvent::PullRequest | WorkflowEvent::Release => false,
    };
    let branches = if context.branch_filters.is_empty() {
        &connection.source_policy.branches
    } else {
        &context.branch_filters
    };
    if branch_event {
        for branch in branches {
            if !is_literal_filter(branch) {
                return Err(HostError::Config);
            }
            if (connection.source_policy.mode == crate::config::SourcePolicyMode::Repository
                || connection.source_policy.branches.contains(branch))
                && (context.branch_filters.is_empty() || context.branch_filters.contains(branch))
            {
                refs.push(format!("refs/heads/{branch}"));
            }
        }
    }

    if matches!(event, WorkflowEvent::Push | WorkflowEvent::WorkflowDispatch) {
        if !context.branch_filters.is_empty() && !context.tag_filters.is_empty() {
            return Err(HostError::Config);
        }
        if context.branch_filters.is_empty()
            && context.tag_filters.is_empty()
            && (connection.source_policy.mode == crate::config::SourcePolicyMode::Repository
                || !connection.source_policy.tag_prefixes.is_empty())
        {
            return Err(HostError::Config);
        }
        for tag in &context.tag_filters {
            if !is_literal_filter(tag) {
                return Err(HostError::Config);
            }
            let event_ref = format!("refs/tags/{tag}");
            if connection.source_policy.mode == crate::config::SourcePolicyMode::Repository
                || connection
                    .source_policy
                    .tag_prefixes
                    .iter()
                    .any(|prefix| tag.starts_with(prefix))
            {
                refs.push(event_ref);
            }
        }
    }

    if event == WorkflowEvent::Release {
        return Err(HostError::Config);
    }
    refs.sort();
    refs.dedup();
    Ok(refs)
}

#[cfg(test)]
#[path = "inventory_bridge_tests.rs"]
mod tests;

fn parse_event(event: &str) -> Result<WorkflowEvent, HostError> {
    match event {
        "pull_request" => Ok(WorkflowEvent::PullRequest),
        "push" => Ok(WorkflowEvent::Push),
        "merge_group" => Ok(WorkflowEvent::MergeGroup),
        "schedule" => Ok(WorkflowEvent::Schedule),
        "workflow_dispatch" => Ok(WorkflowEvent::WorkflowDispatch),
        "release" => Ok(WorkflowEvent::Release),
        _ => Err(HostError::Config),
    }
}

fn is_literal_filter(value: &str) -> bool {
    !value.is_empty()
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']' | b'!'))
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JobAuthorityInventory {
    schema_version: u32,
    generator_source_sha: String,
    inventory_sha256: String,
    workflows: Vec<InventoryWorkflow>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InventoryWorkflow {
    path: String,
    content_sha256: String,
    calls: Vec<InventoryCall>,
    jobs: Vec<InventoryJob>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InventoryCall {
    path: String,
    content_sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InventoryJob {
    root_path: String,
    workflow_path: String,
    job_key: String,
    role: JobAuthorityRole,
    contexts: Vec<InventoryContext>,
    permissions_declared: bool,
    permissions: BTreeMap<String, String>,
    secrets: Vec<String>,
    environments: Vec<String>,
    condition_sha256: Option<String>,
    concurrency: Option<InventoryConcurrency>,
    needs: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InventoryContext {
    event: String,
    branch_filters: Vec<String>,
    tag_filters: Vec<String>,
    pull_request_base_branches: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InventoryConcurrency {
    scope: String,
    group: String,
    cancel_in_progress: String,
}

#[derive(Serialize)]
struct InventoryContent<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    workflows: &'a [InventoryWorkflow],
}
