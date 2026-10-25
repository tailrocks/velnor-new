use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sha2::{Digest, Sha256};
use velnor_actions_contract::config::{JobAuthorityBinding, JobAuthorityRole};
use velnor_actions_workflow_renderer::{
    AuthorityConcurrency, AuthorityJob, AuthorityTrigger, AuthorityWorkflow,
    extract_workflow_authority,
};

use crate::OrchestratorError;
pub use crate::job_authority_inventory_types::{
    InventoryCall, InventoryConcurrency, InventoryContext, InventoryJob, InventoryWorkflow,
    JobAuthorityInventory,
};
use crate::prepare::GenerationPreparation;

const INVENTORY_SCHEMA: u32 = 1;

#[path = "job_authority_source.rs"]
mod source;

#[derive(Serialize)]
struct InventoryContent<'a> {
    schema_version: u32,
    generator_source_sha: &'a str,
    workflows: &'a [InventoryWorkflow],
}

/// Build an authority inventory from the final rendered workflow tree.
///
/// # Errors
///
/// The function rejects incomplete role maps and unsupported workflow facts.
pub fn generate_job_authority_inventory(
    preparation: &GenerationPreparation,
) -> Result<JobAuthorityInventory, OrchestratorError> {
    let tree = crate::generate::render_staged_tree(preparation)?;
    let facts = extract_workflow_authority(&tree.files)?;
    let source_sha =
        source::generator_source_sha(&preparation.root, preparation.config.workflow.policy)?;
    build_from_facts(
        &facts,
        &tree.files,
        &preparation.config.job_authority,
        source_sha,
    )
}

fn build_from_facts(
    facts: &[AuthorityWorkflow],
    files: &[velnor_actions_workflow_renderer::RenderedFile],
    bindings: &[JobAuthorityBinding],
    generator_source_sha: String,
) -> Result<JobAuthorityInventory, OrchestratorError> {
    if facts.iter().any(|workflow| !workflow.calls.is_empty()) {
        return Err(inventory_error(
            "reusable_workflow_authority_composition_unsupported",
        ));
    }
    let by_path: BTreeMap<&str, &AuthorityWorkflow> = facts
        .iter()
        .map(|workflow| (workflow.path.as_str(), workflow))
        .collect();
    let file_hashes = workflow_hashes(facts, files)?;
    let roles = role_map(bindings)?;
    let traversal = traverse_roots(facts, &by_path, &roles)?;
    if traversal.reached_workflows.len() != facts.len() {
        return Err(inventory_error("workflow_outside_trigger_closure"));
    }
    if traversal.used_roles.len() != roles.len() {
        return Err(inventory_error("job_role_binding_unmatched"));
    }
    let workflows = assemble_workflows(facts, &file_hashes, traversal.jobs_by_workflow)?;
    let content = InventoryContent {
        schema_version: INVENTORY_SCHEMA,
        generator_source_sha: &generator_source_sha,
        workflows: &workflows,
    };
    let canonical =
        serde_json::to_vec(&content).map_err(|_| inventory_error("inventory_encode_failed"))?;
    Ok(JobAuthorityInventory {
        schema_version: INVENTORY_SCHEMA,
        generator_source_sha,
        inventory_sha256: sha256(&canonical),
        workflows,
    })
}

struct Traversal {
    jobs_by_workflow: BTreeMap<String, Vec<InventoryJob>>,
    reached_workflows: BTreeSet<String>,
    used_roles: BTreeSet<(String, String, String)>,
}

fn workflow_hashes(
    facts: &[AuthorityWorkflow],
    files: &[velnor_actions_workflow_renderer::RenderedFile],
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    facts
        .iter()
        .map(|workflow| {
            let bytes = files
                .iter()
                .find(|file| file.path == workflow.path)
                .map(|file| file.bytes.as_bytes())
                .ok_or_else(|| inventory_error("workflow_bytes_missing"))?;
            Ok((workflow.path.clone(), sha256(bytes)))
        })
        .collect()
}

fn role_map(
    bindings: &[JobAuthorityBinding],
) -> Result<BTreeMap<(&str, &str, &str), JobAuthorityRole>, OrchestratorError> {
    let roles = bindings
        .iter()
        .map(|binding| {
            (
                (
                    binding.root_path.as_str(),
                    binding.workflow_path.as_str(),
                    binding.job_key.as_str(),
                ),
                binding.role,
            )
        })
        .collect::<BTreeMap<_, _>>();
    if roles.len() != bindings.len() {
        return Err(inventory_error("duplicate_job_role_binding"));
    }
    Ok(roles)
}

fn traverse_roots(
    facts: &[AuthorityWorkflow],
    by_path: &BTreeMap<&str, &AuthorityWorkflow>,
    roles: &BTreeMap<(&str, &str, &str), JobAuthorityRole>,
) -> Result<Traversal, OrchestratorError> {
    let roots = facts
        .iter()
        .filter(|workflow| !workflow.triggers.is_empty())
        .map(|workflow| workflow.path.as_str())
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Err(inventory_error("workflow_root_missing"));
    }
    let mut traversal = Traversal {
        jobs_by_workflow: BTreeMap::new(),
        reached_workflows: BTreeSet::new(),
        used_roles: BTreeSet::new(),
    };
    for root in roots {
        visit_root(root, by_path, roles, &mut traversal)?;
    }
    Ok(traversal)
}

fn visit_root(
    root: &str,
    by_path: &BTreeMap<&str, &AuthorityWorkflow>,
    roles: &BTreeMap<(&str, &str, &str), JobAuthorityRole>,
    traversal: &mut Traversal,
) -> Result<(), OrchestratorError> {
    let root_facts = by_path
        .get(root)
        .ok_or_else(|| inventory_error("workflow_root_missing"))?;
    let contexts = contexts(&root_facts.triggers)?;
    let mut closure = BTreeSet::new();
    visit_workflow(root, by_path, &mut closure)?;
    for path in closure {
        add_workflow_jobs(root, &path, by_path, roles, &contexts, traversal)?;
    }
    Ok(())
}

fn add_workflow_jobs(
    root: &str,
    path: &str,
    by_path: &BTreeMap<&str, &AuthorityWorkflow>,
    roles: &BTreeMap<(&str, &str, &str), JobAuthorityRole>,
    contexts: &[InventoryContext],
    traversal: &mut Traversal,
) -> Result<(), OrchestratorError> {
    traversal.reached_workflows.insert(path.to_owned());
    let workflow = by_path
        .get(path)
        .ok_or_else(|| inventory_error("workflow_target_missing"))?;
    for job in &workflow.jobs {
        let key = (root, path, job.job_key.as_str());
        let role = roles.get(&key).copied().ok_or_else(|| {
            inventory_error(&format!("job_role_missing:{}:{}", path, job.job_key))
        })?;
        traversal
            .used_roles
            .insert((root.to_owned(), path.to_owned(), job.job_key.clone()));
        traversal
            .jobs_by_workflow
            .entry(path.to_owned())
            .or_default()
            .push(inventory_job(root, path, job, role, contexts)?);
    }
    Ok(())
}

fn assemble_workflows(
    facts: &[AuthorityWorkflow],
    file_hashes: &BTreeMap<String, String>,
    mut jobs_by_workflow: BTreeMap<String, Vec<InventoryJob>>,
) -> Result<Vec<InventoryWorkflow>, OrchestratorError> {
    let mut workflows = Vec::new();
    for fact in facts {
        let content_sha256 = file_hashes
            .get(fact.path.as_str())
            .cloned()
            .ok_or_else(|| inventory_error("workflow_digest_missing"))?;
        let mut calls = fact
            .calls
            .iter()
            .map(|call| {
                let digest = file_hashes
                    .get(call.path.as_str())
                    .cloned()
                    .ok_or_else(|| inventory_error("workflow_call_target_missing"))?;
                Ok(InventoryCall {
                    path: call.path.clone(),
                    content_sha256: digest,
                })
            })
            .collect::<Result<Vec<_>, OrchestratorError>>()?;
        calls.sort();
        let mut jobs = jobs_by_workflow.remove(&fact.path).unwrap_or_default();
        jobs.sort_by(|left, right| {
            (&left.root_path, &left.workflow_path, &left.job_key).cmp(&(
                &right.root_path,
                &right.workflow_path,
                &right.job_key,
            ))
        });
        workflows.push(InventoryWorkflow {
            path: fact.path.clone(),
            content_sha256,
            calls,
            jobs,
        });
    }
    Ok(workflows)
}

fn inventory_job(
    root: &str,
    workflow_path: &str,
    job: &AuthorityJob,
    role: JobAuthorityRole,
    contexts: &[InventoryContext],
) -> Result<InventoryJob, OrchestratorError> {
    if !job.permissions.declared {
        return Err(inventory_error(&format!(
            "job_permissions_undeclared:{workflow_path}:{}",
            job.job_key
        )));
    }
    let supported_permissions = [
        "actions",
        "attestations",
        "checks",
        "contents",
        "deployments",
        "discussions",
        "id-token",
        "issues",
        "models",
        "packages",
        "pages",
        "pull-requests",
        "repository-projects",
        "security-events",
        "statuses",
    ];
    if job
        .permissions
        .scopes
        .keys()
        .any(|permission| !supported_permissions.contains(&permission.as_str()))
    {
        return Err(inventory_error(&format!(
            "job_permission_scope_unsupported:{workflow_path}:{}",
            job.job_key
        )));
    }
    let condition_sha256 = job
        .condition
        .as_deref()
        .map(|condition| sha256(condition.as_bytes()));
    Ok(InventoryJob {
        root_path: root.to_owned(),
        workflow_path: workflow_path.to_owned(),
        job_key: job.job_key.clone(),
        role,
        contexts: contexts.to_vec(),
        permissions_declared: job.permissions.declared,
        permissions: job.permissions.scopes.clone(),
        secrets: job.secrets.clone(),
        environments: job.environments.clone(),
        condition_sha256,
        concurrency: job.concurrency.as_ref().map(inventory_concurrency),
        needs: job.needs.clone(),
    })
}

fn inventory_concurrency(value: &AuthorityConcurrency) -> InventoryConcurrency {
    InventoryConcurrency {
        scope: value.scope.clone(),
        group: value.group.clone(),
        cancel_in_progress: value.cancel_in_progress.clone(),
    }
}

fn contexts(triggers: &[AuthorityTrigger]) -> Result<Vec<InventoryContext>, OrchestratorError> {
    let mut contexts = triggers
        .iter()
        .map(|trigger| {
            if trigger.event == "pull_request" && !trigger.branch_filters.is_empty() {
                return Err(inventory_error("pull_request_branch_filter_invalid"));
            }
            Ok(InventoryContext {
                event: trigger.event.clone(),
                branch_filters: trigger.branch_filters.clone(),
                tag_filters: trigger.tag_filters.clone(),
                pull_request_base_branches: trigger.pull_request_base_branches.clone(),
            })
        })
        .collect::<Result<Vec<_>, OrchestratorError>>()?;
    contexts.sort();
    Ok(contexts)
}

fn visit_workflow(
    path: &str,
    workflows: &BTreeMap<&str, &AuthorityWorkflow>,
    visited: &mut BTreeSet<String>,
) -> Result<(), OrchestratorError> {
    if !visited.insert(path.to_owned()) {
        return Ok(());
    }
    let workflow = workflows
        .get(path)
        .ok_or_else(|| inventory_error(&format!("workflow_call_target_missing:{path}")))?;
    for call in &workflow.calls {
        visit_workflow(&call.path, workflows, visited)?;
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = Vec::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(HEX[usize::from(byte >> 4)]);
        output.push(HEX[usize::from(byte & 0x0f)]);
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn inventory_error(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("job_authority_inventory:{problem}"),
    }
}

#[cfg(test)]
#[path = "job_authority_inventory_tests.rs"]
mod tests;
