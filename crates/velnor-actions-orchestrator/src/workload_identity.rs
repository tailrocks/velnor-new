//! Native workload identity; repository content binds while undeclared reads forbid reuse.

use std::collections::BTreeSet;

use velnor_actions_contract::{
    ClosureBuilder, PlanPackage, ProposedTask, Provenance, Stack, StackExtension, TaskInputClosure,
};

use super::identities::ExtensionBundle;
use crate::discover::Discovery;

#[path = "workload_action.rs"]
pub(crate) mod action;

/// Native extension remains unqualified for reuse and baseline coverage.
pub(crate) fn extension_for(
    task: &ProposedTask,
    bundle: &ExtensionBundle,
) -> Result<StackExtension, velnor_actions_contract::ContractError> {
    let transport = if task.configuration == "docker_build" {
        Some(action::DockerTransport::new(
            &task.identity.project_root,
            &task.configuration,
        )?)
    } else {
        None
    };
    Ok(StackExtension {
        schema: "workload-task-identity-v1".to_owned(),
        data: serde_json::json!({
            "name": task.identity.unit_key,
            "kind": task.configuration,
            "root": task.identity.project_root,
            "inputs": task.identity.declared_inputs,
            "payload": task.payload,
            "graph_digest": bundle.graph_digest(),
            "config_digest": bundle.config_digest(),
            "undeclared_reads": true,
            "action_transport": transport,
        }),
    })
}

/// Full repository closure, plus an explicit native execution unknown.
pub(crate) fn closure(
    task: &ProposedTask,
    checkout: &Provenance,
    graph: &str,
    toolchain: &str,
    platform: &str,
) -> TaskInputClosure {
    ClosureBuilder::new()
        .input("repository", checkout.clone())
        .input(
            "native_execution",
            Provenance::Unknown {
                reason: "native_tool_undeclared_reads".to_owned(),
            },
        )
        .digest("graph", graph)
        .digest("toolchain", toolchain)
        .digest("platform", platform)
        .value("root", &task.identity.project_root)
        .value("configuration", &task.configuration)
        .value("kind", &task.task_kind)
        .build(&task.task_id)
}

/// Adapter display metadata without Rust driver parsing.
pub(crate) fn metadata(
    task: &ProposedTask,
) -> Result<serde_json::Value, velnor_actions_contract::ContractError> {
    let mut metadata = serde_json::json!({
        "package": task.identity.unit_key,
        "manifest": task.identity.unit_path,
        "configuration": task.configuration,
        "kind": task.task_kind,
        "compile_driver": "native",
        "test_runner": "native",
        "task_cache_enabled": false,
    });
    if task.configuration == "docker_build" {
        metadata["action"] = serde_json::to_value(action::binding(task)?).map_err(|error| {
            velnor_actions_contract::ContractError::CanonicalJson(error.to_string())
        })?;
    }
    Ok(metadata)
}

/// Exact native tool selectors; Docker's runner tool remains unqualified.
pub(crate) fn toolchain_inputs(
    task: &ProposedTask,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<
    velnor_actions_contract::cachekey::ToolchainInputs,
    velnor_actions_contract::ContractError,
> {
    let mut tools = catalog
        .tool_specs(&crate::workloads::tools(&task.configuration)?)
        .map_err(|error| {
            velnor_actions_contract::ContractError::identity("native_catalog", error.to_string())
        })?;
    tools.sort();
    Ok(velnor_actions_contract::cachekey::ToolchainInputs {
        tools,
        components: Vec::new(),
        compile_driver: task.identity.compile_driver.clone(),
        test_runner: task.identity.test_runner.clone(),
    })
}

/// One inventory row per configured workload, including unselected tasks.
pub(crate) fn packages(discovery: &Discovery, selected: &BTreeSet<&str>) -> Vec<PlanPackage> {
    let mut units = BTreeSet::new();
    discovery
        .proposals
        .iter()
        .filter(|task| Stack::from_id(&task.stack_id) == Some(Stack::Workload))
        .filter(|task| units.insert(task.identity.unit_id.clone()))
        .map(|task| {
            super::package_row(
                &task.identity.unit_id,
                &task.identity.unit_key,
                &task.identity.unit_path,
                discovery,
                selected,
            )
        })
        .collect()
}
