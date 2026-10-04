//! Compiled owner for the cached OpenTofu init obligation.

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, ProposedTask, SourceBoundHelper, SourceBoundOperation,
    StepKind, WorkflowIr,
};
use velnor_actions_mise::{
    OPENTOFU_VERSION, PinnedTool, ToolCatalog, catalog::qualification::DistributionHost,
};

use crate::OrchestratorError;

const BODY: &str = include_str!("tofu_cached_init_source.sh");

/// Build the one owner record for a root's exact cached-init task.
pub(crate) fn from_task_id(
    task_id: &str,
    catalog: &ToolCatalog,
    host: DistributionHost,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let root = root_from_task_id(task_id)?;
    let distribution = catalog.native_distribution(host, PinnedTool::Opentofu)?;
    distribution.required_install_plan()?;
    let binary_path = distribution.required_installed_binary_path()?;
    let tofu_version = catalog.version(PinnedTool::Opentofu);
    if tofu_version != OPENTOFU_VERSION
        || velnor_actions_mise::validate_exact_version("opentofu", tofu_version).is_err()
    {
        return Err(contract("tofu_cached_init_catalog"));
    }
    if version != env!("CARGO_PKG_VERSION") {
        return Err(contract("tofu_cached_init_version"));
    }
    let source = velnor_actions_contract::generated_source(version, BODY)?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::TofuCachedInit;
    let owner = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let root_arg = if root.is_empty() { "." } else { &root };
    let invocation = HelperInvocation::compiled(
        owner,
        vec![
            root_arg.to_owned(),
            tofu_version.to_owned(),
            velnor_actions_tofu::key_for_root(&root),
            binary_path.to_owned(),
        ],
        catalog.native_tool_specs(host, &[PinnedTool::Opentofu])?,
    )?;
    let mut environment = crate::tofu_config_step::task_env(&root)?;
    environment.insert(
        velnor_actions_mise::runtime_paths::MISE_DATA_DIR_ENV.to_owned(),
        velnor_actions_mise::runtime_paths::MISE_DATA_DIR.to_owned(),
    );
    Ok(CompiledSourceHelper::compiled(invocation, source)?.with_environment(environment))
}

/// Qualify proposal facts before binding the same runtime source recipe.
pub(crate) fn from_proposal(
    task: &ProposedTask,
    catalog: &ToolCatalog,
    host: DistributionHost,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    velnor_actions_tofu::normalized_root_for_proposal(task)?;
    let kind = velnor_actions_tofu::TofuTaskKind::InitForValidate;
    if task.task_kind != kind.as_str() || task.configuration != "default" {
        return Err(contract("tofu_cached_init_proposal_binding"));
    }
    from_task_id(&task.task_id, catalog, host, version)
}

/// Reconstruct final helper records through the closed semantic source owner.
pub(crate) fn collect_records(
    ir: &WorkflowIr,
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    collect_job_records(ir.jobs.values(), version)
}

pub(crate) fn collect_job_records<'a>(
    jobs: impl IntoIterator<Item = &'a velnor_actions_contract::Job>,
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let mut records = Vec::new();
    for job in jobs {
        for step in &job.steps {
            let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
                continue;
            };
            if invocation.descriptor().operation() != SourceBoundOperation::TofuCachedInit {
                continue;
            }
            let host = crate::workloads::host_for_runner(&job.runs_on)?;
            let [root, _pin, _root_key, _binary_path] = invocation.args() else {
                return Err(contract("tofu_cached_init_invocation"));
            };
            let root = if root == "." { "" } else { root };
            let task_id = velnor_actions_tofu::task_id_for_root(
                root,
                velnor_actions_tofu::TofuTaskKind::InitForValidate,
                "default",
            )?;
            let record = from_task_id(&task_id, &ToolCatalog::pinned(), host, version)?;
            if record.invocation() != invocation || record.environment() != env {
                return Err(contract("tofu_cached_init_record_binding"));
            }
            if !records.contains(&record) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn root_from_task_id(task_id: &str) -> Result<String, OrchestratorError> {
    velnor_actions_contract::validate_task_id(task_id).map_err(OrchestratorError::from)?;
    if crate::extension_schemas::task_stack_segment(task_id) != Some("tofu") {
        return Err(contract("tofu_cached_init_stack"));
    }
    if crate::extension_schemas::task_kind_segment(task_id) != Some("init") {
        return Err(contract("tofu_cached_init_kind"));
    }
    if velnor_actions_contract::split_shard_suffix(task_id).is_some() {
        return Err(contract("tofu_cached_init_shard"));
    }
    if task_id.rsplit('/').next() != Some("default") {
        return Err(contract("tofu_cached_init_configuration"));
    }
    let key = crate::extension_schemas::task_key_segment(task_id)
        .ok_or_else(|| contract("tofu_cached_init_key"))?;
    let root = velnor_actions_tofu::root_for_key(&key)?;
    velnor_actions_tofu::validate_normalized_root(&root).map_err(OrchestratorError::from)?;
    velnor_actions_tofu::tofu_payload_argv(
        velnor_actions_tofu::TofuTaskKind::InitForValidate,
        &root,
    )
    .map_err(OrchestratorError::from)?;
    Ok(root)
}

fn contract(problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.into(),
    }
}

#[cfg(test)]
#[path = "tofu_cached_init_tests.rs"]
mod tests;
