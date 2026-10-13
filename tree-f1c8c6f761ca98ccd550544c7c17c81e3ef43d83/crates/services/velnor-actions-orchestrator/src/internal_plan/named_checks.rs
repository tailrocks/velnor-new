//! Identity and transport for explicit opaque checks.
pub(crate) mod plan;
use crate::discover::Discovery;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoBytes, read_repo_bytes};
use std::path::Path;
use velnor_actions_contract::cachekey::ToolchainInputs;
use velnor_actions_contract::{
    ClosureBuilder, ContractError, NamedCheckIdentityExtension, ProposedTask, Provenance, Stack,
    StackExtension, TaskInputClosure, digest_b3,
};
use velnor_actions_mise::{DiscoveredCheck, ToolCatalog};

/// Require the discovered definition corresponding to the exact proposal.
pub(crate) fn discovered<'a>(
    discovery: &'a Discovery,
    task: &ProposedTask,
) -> Result<&'a DiscoveredCheck, ContractError> {
    require_mise(task)?;
    discovery
        .mise_checks
        .iter()
        .find(|item| item.proposal.task_id == task.task_id)
        .ok_or_else(|| ContractError::identity("check", "undiscovered_check"))
}
fn require_mise(task: &ProposedTask) -> Result<(), ContractError> {
    if Stack::require_known(&task.stack_id)? != Stack::Mise {
        return Err(ContractError::identity("check", "wrong_stack"));
    }
    Ok(())
}
/// Typed extension binds the complete definition and resolved task configuration.
pub(crate) fn extension_for(item: &DiscoveredCheck) -> Result<StackExtension, ContractError> {
    require_mise(&item.proposal)?;
    item.verify_qualification_identity()
        .map_err(|e| ContractError::identity("qualification", e.to_string()))?;
    NamedCheckIdentityExtension {
        check: item.check.clone(),
        task_config_digest: digest_b3(item.task_config.as_bytes()),
        qualification_digest: item.qualification_digest.clone(),
    }
    .to_stack_extension()
}
/// Opaque execution metadata forwarded to the existing matrix transport.
pub(crate) fn metadata_for(item: &DiscoveredCheck) -> Result<serde_json::Value, ContractError> {
    require_mise(&item.proposal)?;
    item.verify_qualification_identity()
        .map_err(|e| ContractError::identity("qualification", e.to_string()))?;
    serde_json::to_value(item.entry_metadata())
        .map_err(|e| ContractError::CanonicalJson(e.to_string()))
}
/// Adapter-owned pinned tool evidence encoded in neutral proposal flags.
pub(crate) fn toolchain_inputs(
    task: &ProposedTask,
    _catalog: &ToolCatalog,
) -> Result<ToolchainInputs, ContractError> {
    require_mise(task)?;
    let mut tools: Vec<String> = task
        .identity
        .flags
        .iter()
        .filter_map(|flag| flag.strip_prefix("tool:").map(str::to_owned))
        .collect();
    tools.push(format!("mise@{}", velnor_actions_mise::MISE_VERSION));
    tools.sort();
    tools.dedup();
    Ok(ToolchainInputs {
        tools,
        components: task
            .identity
            .flags
            .iter()
            .filter(|flag| flag.starts_with("qualified_tools:"))
            .cloned()
            .collect(),
        compile_driver: "mise".to_owned(),
        test_runner: "mise".to_owned(),
    })
}
/// Bind declared source bytes while retaining the opaque-state refusal.
pub(crate) fn resolve_closure(
    root: &Path,
    task: &ProposedTask,
    graph: &str,
    toolchain: &str,
    platform: &str,
) -> Result<TaskInputClosure, ContractError> {
    resolve_closure_until(root, task, graph, toolchain, platform, None)
}

/// Resolve declared byte inputs while consuming an optional check deadline.
pub(crate) fn resolve_closure_until(
    root: &Path,
    task: &ProposedTask,
    graph: &str,
    toolchain: &str,
    platform: &str,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<TaskInputClosure, ContractError> {
    require_mise(task)?;
    let mut builder = ClosureBuilder::new()
        .digest("graph", graph)
        .digest("toolchain", toolchain)
        .digest("platform", platform)
        .input(
            "opaque_task_state",
            Provenance::Unknown {
                reason: "undeclared_inputs".to_owned(),
            },
        );
    for path in &task.identity.declared_inputs {
        let read = if let Some(deadline) = deadline {
            crate::safe_read::read_repo_bytes_until(root, path, MAX_REPO_FILE_BYTES, deadline)
        } else {
            read_repo_bytes(root, path, MAX_REPO_FILE_BYTES)
        }
        .map_err(|e| ContractError::identity("check_input", e.to_string()))?;
        let provenance = match read {
            RepoBytes::Bytes(bytes) => Provenance::Known {
                digest: digest_b3(&bytes),
            },
            RepoBytes::Absent => Provenance::AbsentProven {
                evidence: "checkout_absent".to_owned(),
            },
        };
        builder = builder.input(&format!("file:{path}"), provenance);
    }
    let request = velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()]);
    let head = if let Some(deadline) = deadline {
        Some(
            request
                .run_in_until(root, deadline)
                .map_err(|error| ContractError::identity("check_head", error.to_string()))?,
        )
    } else {
        request.run_in(root).ok()
    };
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| ContractError::identity("check_head", error.to_string()))?;
    }
    let provenance = match head {
        Some(output) if output.success => {
            let sha = output
                .stdout_text("git")
                .map_err(|e| ContractError::identity("check_head", e.to_string()))?;
            Provenance::Known {
                digest: digest_b3(sha.trim().as_bytes()),
            }
        }
        _ => Provenance::Unknown {
            reason: "checkout_head_unresolved".to_owned(),
        },
    };
    builder = builder.input("checkout_head", provenance);
    Ok(builder.build(&task.task_id))
}

#[cfg(test)]
mod tests;
