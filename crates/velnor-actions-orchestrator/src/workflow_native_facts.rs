// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{PreparedWorkflow, WorkflowBuildInput};
use crate::OrchestratorError;
use velnor_actions_bun::member_job_scripts;
use velnor_actions_contract::Stack;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::native_authority::{
    CargoWorkspaceFact, GeneratedSourceFact, GradleRootFact, NativeToolOrigin, NativeToolPin,
    RustTaskFact, TofuTaskFact, WorkflowSourceFact, WorkspaceFormatFact,
};

pub(super) fn tool_pins(
    config: &velnor_actions_contract::VelnorConfig,
    catalog: &ToolCatalog,
) -> Vec<NativeToolPin> {
    PinnedTool::ALL
        .iter()
        .filter_map(|tool| {
            let origin = if catalog.is_project_declared(*tool) {
                NativeToolOrigin::ProjectDeclaration
            } else if configured_tool(config, *tool).is_some() {
                NativeToolOrigin::ConfigOverride
            } else {
                return None;
            };
            Some(NativeToolPin {
                tool: *tool,
                version: catalog.version(*tool).to_owned(),
                origin,
            })
        })
        .collect()
}

pub(super) fn cargo_workspaces(input: &WorkflowBuildInput<'_>) -> Vec<CargoWorkspaceFact> {
    input
        .discovery
        .workspaces
        .iter()
        .map(|workspace| {
            let root = workspace.record.workspace_root.clone();
            let lock = crate::discover::workspace_lock(&root);
            CargoWorkspaceFact {
                root,
                test_runner: workspace.profile.test_runner,
                lockfile_path: input.root.join(&lock).is_file().then_some(lock),
            }
        })
        .collect()
}

pub(super) fn workspace_format(
    input: &WorkflowBuildInput<'_>,
) -> Result<Option<WorkspaceFormatFact>, OrchestratorError> {
    input
        .discovery
        .proposals
        .iter()
        .find(|task| {
            velnor_actions_rust::is_workspace_fmt_task(
                &task.task_kind,
                &task.identity.unit_id,
                &task.display_name,
            )
        })
        .map(|task| {
            Ok::<_, velnor_actions_contract::ContractError>(WorkspaceFormatFact {
                manifest: task.identity.unit_path.clone(),
                configuration: task.configuration.clone(),
                target: task.identity.target.clone(),
                compile_driver: velnor_actions_rust::CompileDriver::parse(
                    &task.identity.compile_driver,
                )?,
            })
        })
        .transpose()
        .map_err(OrchestratorError::from)
}

pub(super) fn gradle_roots(input: &WorkflowBuildInput<'_>) -> Vec<GradleRootFact> {
    input
        .discovery
        .gradle_roots
        .iter()
        .filter(|_| input.config.stacks.gradle.is_some())
        .map(|root| GradleRootFact {
            root: root.root.clone(),
            projects: root.projects.clone(),
        })
        .collect()
}

pub(super) fn rust_tasks(
    input: &WorkflowBuildInput<'_>,
) -> Result<Vec<RustTaskFact>, OrchestratorError> {
    input
        .discovery
        .proposals
        .iter()
        .filter(|task| Stack::from_id(&task.stack_id) == Some(Stack::Rust))
        .map(|task| {
            Ok(RustTaskFact {
                unit: task.identity.unit_id.clone(),
                no_targets: task.no_targets,
                manifest: task.identity.unit_path.clone(),
                kind: velnor_actions_rust::TaskKind::parse(&task.task_kind)?,
                configuration: task.configuration.clone(),
                all_features: task
                    .identity
                    .flags
                    .iter()
                    .any(|flag| flag == "--all-features"),
            })
        })
        .collect::<Result<Vec<_>, velnor_actions_contract::ContractError>>()
        .map_err(OrchestratorError::from)
}

pub(super) fn tofu_tasks(
    input: &WorkflowBuildInput<'_>,
) -> Result<Vec<TofuTaskFact>, OrchestratorError> {
    input
        .discovery
        .proposals
        .iter()
        .filter(|task| Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
        .map(|task| {
            Ok(TofuTaskFact {
                root: velnor_actions_tofu::normalized_root_for_proposal(task)?.to_owned(),
                kind: velnor_actions_tofu::TofuTaskKind::parse(&task.task_kind)?,
                configuration: task.configuration.clone(),
            })
        })
        .collect::<Result<Vec<_>, velnor_actions_contract::ContractError>>()
        .map_err(OrchestratorError::from)
}

pub(super) fn generated_sources(
    input: &WorkflowBuildInput<'_>,
    prepared: &PreparedWorkflow,
) -> Result<Vec<GeneratedSourceFact>, OrchestratorError> {
    let mut generated_sources = Vec::new();
    for root in &input.discovery.bun.roots {
        let bun_config = input.config.stacks.bun.as_ref();
        let private_assets = prepared
            .private
            .as_ref()
            .map(|private| private.steps_for_bun(&root.workspace.root))
            .transpose()?
            .is_some_and(|steps| !steps.is_empty());
        for member in &root.workspace.members {
            let has_member_job =
                bun_config.is_some_and(|bun| !matches!(member_job_scripts(member, bun), Ok(None)));
            let has_suites = bun_config
                .and_then(|bun| bun.members.get(&member.name))
                .is_some_and(|config| !config.suites.is_empty());
            generated_sources.push(GeneratedSourceFact::BunMember {
                name: member.name.clone(),
                workspace_root: root.workspace.root.clone(),
                scripts: member.scripts.iter().cloned().collect(),
                private_assets: private_assets && (has_member_job || has_suites),
            });
        }
    }
    generated_sources.extend(input.discovery.dbt.projects.iter().map(|project| {
        GeneratedSourceFact::Dbt {
            root: project.dir.clone(),
        }
    }));
    generated_sources.extend(input.discovery.ansible.roots.iter().map(|root| {
        GeneratedSourceFact::Ansible {
            root: root.dir.clone(),
        }
    }));
    Ok(generated_sources)
}

pub(super) fn workflow_sources(
    input: &WorkflowBuildInput<'_>,
    prepared: &PreparedWorkflow,
) -> Vec<WorkflowSourceFact> {
    let mut source_paths = std::collections::BTreeSet::from([
        "mise.toml".to_owned(),
        "mise.lock".to_owned(),
        "rust-toolchain.toml".to_owned(),
    ]);
    for task in &input.config.workflow.tasks {
        let source = match task {
            velnor_actions_contract::WorkflowTask::Verification(task) => Some(&task.source),
            velnor_actions_contract::WorkflowTask::Build(task) => Some(&task.source),
            velnor_actions_contract::WorkflowTask::NativeImage(_) => None,
        };
        if let Some(source) = source {
            source_paths.insert(source.mise_config.clone());
            source_paths.insert(source.mise_lock_path());
            source_paths.insert(source.rust_toolchain_path());
        }
    }
    let mut workflow_sources = input
        .discovery
        .tool_checks
        .iter()
        .filter(|check| source_paths.contains(&check.path))
        .map(|check| WorkflowSourceFact {
            path: check.path.clone(),
            sha256: check.native.as_ref().map(|native| native.sha256.clone()),
        })
        .collect::<Vec<_>>();
    for policy in &prepared.workflow_tasks {
        if let velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy::NativeImage(policy) = policy { workflow_sources.push(WorkflowSourceFact { path: policy.task.script.clone(), sha256: Some(policy.source_sha256.clone()) }); }
    }
    workflow_sources
}

fn configured_tool(
    config: &velnor_actions_contract::VelnorConfig,
    tool: PinnedTool,
) -> Option<&str> {
    if tool == PinnedTool::Rust {
        config
            .stacks
            .rust
            .as_ref()
            .and_then(|rust| rust.toolchain.as_deref())
            .or_else(|| config.tools.get("rust"))
    } else if tool == PinnedTool::Nextest {
        config.tools.get("cargo-nextest")
    } else {
        config.tools.get(tool.tool_name())
    }
}
