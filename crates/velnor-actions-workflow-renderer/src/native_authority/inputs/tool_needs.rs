// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, admit_manifest, invalid,
};

pub(super) fn validate(input: &NativeAuthorityInput<'_>) -> Result<(), NativeAuthorityError> {
    let rust_config = input
        .config
        .stacks
        .rust
        .clone()
        .unwrap_or_else(velnor_actions_contract::RustStackConfig::default_config);
    validate_configuration(input, &rust_config)?;
    validate_inventory(input, &rust_config)?;
    validate_tools(input)
}

fn validate_configuration(
    input: &NativeAuthorityInput<'_>,
    rust_config: &velnor_actions_contract::RustStackConfig,
) -> Result<(), NativeAuthorityError> {
    if let Some(format) = input.workspace_format {
        admit_manifest(&format.manifest, input.cargo_workspaces, true)?;
        if !rust_config.configurations.iter().any(|configuration| {
            configuration.name == format.configuration && configuration.target == format.target
        }) {
            return Err(invalid("native_workspace_format_configuration"));
        }
    }
    for task in input.rust_tasks {
        admit_manifest(&task.manifest, input.cargo_workspaces, false)?;
        {
            let configuration = rust_config
                .configurations
                .iter()
                .find(|configuration| configuration.name == task.configuration)
                .ok_or_else(|| invalid("native_rust_task_configuration"))?;
            let expected_all_features = !task.unit.is_empty()
                && (configuration.all_features
                    || task.kind == velnor_actions_rust::TaskKind::Clippy
                        && rust_config.clippy_all_features);
            if task.all_features != expected_all_features {
                return Err(invalid("native_rust_task_features"));
            }
        }
    }
    Ok(())
}

fn validate_inventory(
    input: &NativeAuthorityInput<'_>,
    rust_config: &velnor_actions_contract::RustStackConfig,
) -> Result<(), NativeAuthorityError> {
    for task in input
        .rust_tasks
        .iter()
        .filter(|task| !task.no_targets && !task.unit.is_empty())
    {
        if !input.logical_jobs.iter().any(|job| matches!(&job.source, LogicalJobSource::Task(source) if source.unit == task.unit && source.configuration == task.configuration && source.rust_manifest.as_deref() == Some(task.manifest.as_str()))) { return Err(invalid("native_rust_task_inventory")); }
    }
    for source in input
        .logical_jobs
        .iter()
        .filter_map(|job| {
            if let LogicalJobSource::Task(source) = &job.source {
                Some(source)
            } else {
                None
            }
        })
        .filter(|source| source.rust_manifest.is_some())
    {
        if !input.rust_tasks.iter().any(|task| {
            !task.no_targets
                && task.unit == source.unit
                && task.configuration == source.configuration
                && Some(task.manifest.as_str()) == source.rust_manifest.as_deref()
        }) {
            return Err(invalid("native_rust_task_inventory"));
        }
        if let Some(driver) = rust_config.compile_driver {
            let expected = match driver {
                velnor_actions_contract::DeclaredCompileDriver::Cargo => {
                    velnor_actions_rust::CompileDriver::Cargo
                }
                velnor_actions_contract::DeclaredCompileDriver::Mbx => {
                    velnor_actions_rust::CompileDriver::Mbx
                }
            };
            if source.compile_driver != Some(expected) {
                return Err(invalid("native_rust_driver_configuration"));
            }
        }
    }
    if let Some(runner) = rust_config.test_runner {
        let expected = match runner {
            velnor_actions_contract::DeclaredTestRunner::CargoTest => {
                velnor_actions_rust::TestRunner::CargoTest
            }
            velnor_actions_contract::DeclaredTestRunner::CargoNextest => {
                velnor_actions_rust::TestRunner::CargoNextest
            }
        };
        if input
            .cargo_workspaces
            .iter()
            .any(|workspace| workspace.test_runner != expected)
        {
            return Err(invalid("native_rust_runner_configuration"));
        }
    }
    Ok(())
}

fn validate_tools(input: &NativeAuthorityInput<'_>) -> Result<(), NativeAuthorityError> {
    validate_tofu_inventory(input)?;
    let all_features = input.rust_tasks.iter().any(|task| task.all_features);
    let nextest = input
        .cargo_workspaces
        .iter()
        .any(|workspace| workspace.test_runner == velnor_actions_rust::TestRunner::CargoNextest);
    if input.tools.all_features != all_features
        || input.tools.nextest != nextest
        || input.tools.opentofu == input.tofu_tasks.is_empty()
    {
        return Err(invalid("native_plan_tool_inventory"));
    }
    let minimum_rust = !input.rust_tasks.is_empty()
        || input.rust_detected
        || !input.cargo_workspaces.is_empty()
        || input.workspace_format.is_some()
        || input.config.workflow.policy
            == velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1;
    let expected_rust = if input.workspace_format.is_some() {
        crate::native_job_builders::PlanRustNeed::CompilerAndComponents
    } else if minimum_rust {
        crate::native_job_builders::PlanRustNeed::Compiler
    } else {
        crate::native_job_builders::PlanRustNeed::None
    };
    if input.tools.rust != expected_rust {
        return Err(invalid("native_plan_tool_needs"));
    }
    let producer =
        input.config.workflow.policy == velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1;
    if input.tools.resolve_qualification_predecessor != producer
        || input.tools.gh != (producer || !input.config.native_checks.is_empty())
    {
        return Err(invalid("native_plan_tool_needs"));
    }
    Ok(())
}

fn validate_tofu_inventory(input: &NativeAuthorityInput<'_>) -> Result<(), NativeAuthorityError> {
    let mut declared = std::collections::BTreeSet::new();
    for task in input.tofu_tasks {
        velnor_actions_tofu::task_id_for_root(&task.root, task.kind, &task.configuration)?;
        if !declared.insert((&task.root, task.kind.as_str(), &task.configuration)) {
            return Err(invalid("native_tofu_task_inventory"));
        }
    }
    let mut logical = std::collections::BTreeSet::new();
    for job in input.logical_jobs {
        if let LogicalJobSource::Task(source) = &job.source {
            for task in &source.tofu {
                velnor_actions_tofu::task_id_for_root(&task.root, task.kind, &task.configuration)?;
                if !logical.insert((&task.root, task.kind.as_str(), &task.configuration)) {
                    return Err(invalid("native_tofu_task_inventory"));
                }
            }
        }
    }
    if declared != logical {
        return Err(invalid("native_tofu_task_inventory"));
    }
    Ok(())
}
