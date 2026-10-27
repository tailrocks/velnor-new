// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{PreparedWorkflow, WorkflowBuildInput, problem};
use crate::OrchestratorError;
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::workflow::crate_job::{reader_job_id_for, writer_job_id_for};
use velnor_actions_contract::{Job, Stack};
use velnor_actions_workflow_renderer::native_authority::{
    GeneratedWorkflowKind, LogicalJobFact, LogicalJobSource, NativeTaskFact, SuitePhase,
    TofuTaskFact,
};

type Sources = BTreeMap<String, LogicalJobSource>;
type TaskGroups<'a> = BTreeMap<String, Vec<&'a velnor_actions_contract::ProposedTask>>;

struct TaskInventory<'a> {
    groups: TaskGroups<'a>,
    seed_ids: BTreeMap<String, String>,
}

pub(super) fn graph(
    input: &WorkflowBuildInput<'_>,
    prepared: &PreparedWorkflow,
    jobs: &BTreeMap<String, Job>,
) -> Result<Vec<LogicalJobFact>, OrchestratorError> {
    let TaskInventory {
        groups: tasks,
        seed_ids,
    } = task_inventory(input)?;
    let mut sources = Sources::new();
    let mut ordered = Vec::new();
    for (id, members) in &tasks {
        let source = task_source(members)?;
        if jobs.contains_key(id) {
            sources.insert(id.clone(), source);
            ordered.push(id.clone());
            continue;
        }
        // A split group leaves reader/writer branches in its place: they take
        // the group's slot so logical order still matches build order.
        for branch in [reader_job_id_for(id), writer_job_id_for(id)] {
            if jobs.contains_key(branch.as_str()) {
                sources.insert(branch.clone(), source.clone());
                ordered.push(branch);
            }
        }
    }
    insert_named_checks(input, &mut sources, &mut ordered);
    insert_gradle(input, &mut sources, &mut ordered);
    insert_suites(input, jobs, &mut sources, &mut ordered);
    for (id, root) in seed_ids {
        sources.insert(id, LogicalJobSource::DependencySeed { root });
    }
    insert_workflow_tasks(prepared, jobs, &mut sources);
    ordered.extend(
        sources
            .keys()
            .filter(|id| !ordered.contains(id))
            .cloned()
            .collect::<Vec<_>>(),
    );
    ordered
        .into_iter()
        .filter(|id| jobs.contains_key(id))
        .map(|id| {
            let job = &jobs[&id];
            let source = sources
                .remove(&id)
                .ok_or_else(|| problem("native_graph_source_missing"))?;
            Ok(LogicalJobFact {
                id,
                runs_on: job.runs_on.clone(),
                check_runner: job.check_runner.clone(),
                needs: job.needs.clone(),
                outputs: job.outputs.clone(),
                source,
            })
        })
        .collect()
}

fn task_inventory<'a>(
    input: &'a WorkflowBuildInput<'_>,
) -> Result<TaskInventory<'a>, OrchestratorError> {
    let owners = crate::crate_job_ids::JobOwners::new(&input.discovery.proposals)?;
    let mut tasks = BTreeMap::<String, Vec<&velnor_actions_contract::ProposedTask>>::new();
    let mut seed_roots = BTreeSet::new();
    for task in input
        .discovery
        .proposals
        .iter()
        .filter(|task| crate::crate_jobs::is_runnable(task))
    {
        if let Some(id) = owners.for_member(task)? {
            if task.identity.compile_driver == "mbx" {
                seed_roots.insert(
                    crate::source_prep::workspace_root_for_manifest(
                        &task.identity.unit_path,
                        input.fetch_roots,
                    )
                    .to_owned(),
                );
            }
            tasks.entry(id).or_default().push(task);
        }
    }
    let seed_workspaces = seed_roots
        .into_iter()
        .map(|root| crate::crate_jobs::dependency_seed::SeedWorkspace {
            root,
            nextest: false,
        })
        .collect::<Vec<_>>();
    let seed_ids = crate::crate_jobs::dependency_seed::unique_seed_job_ids(&seed_workspaces)
        .into_iter()
        .zip(seed_workspaces)
        .map(|(id, workspace)| (id, workspace.root))
        .collect::<BTreeMap<_, _>>();
    Ok(TaskInventory {
        groups: tasks,
        seed_ids,
    })
}

fn task_source(
    members: &[&velnor_actions_contract::ProposedTask],
) -> Result<LogicalJobSource, OrchestratorError> {
    let first = members[0];
    let source = match Stack::from_id(&first.stack_id) {
        Some(Stack::Mise | Stack::Native) => LogicalJobSource::NamedCheck {
            id: first.identity.unit_id.clone(),
        },
        Some(Stack::Rust | Stack::Tofu) => {
            let tofu = members
                .iter()
                .filter(|task| Stack::from_id(&task.stack_id) == Some(Stack::Tofu))
                .map(|task| {
                    Ok(TofuTaskFact {
                        root: velnor_actions_tofu::normalized_root_for_proposal(task)?.to_owned(),
                        kind: velnor_actions_tofu::TofuTaskKind::parse(&task.task_kind)?,
                        configuration: task.configuration.clone(),
                    })
                })
                .collect::<Result<Vec<_>, velnor_actions_contract::ContractError>>()?;
            LogicalJobSource::Task(NativeTaskFact {
                compile_driver: (Stack::from_id(&first.stack_id) == Some(Stack::Rust))
                    .then(|| {
                        velnor_actions_rust::CompileDriver::parse(&first.identity.compile_driver)
                    })
                    .transpose()?,
                unit: first.identity.unit_id.clone(),
                display_name: first.display_name.clone(),
                configuration: first.configuration.clone(),
                rust_manifest: (Stack::from_id(&first.stack_id) == Some(Stack::Rust))
                    .then(|| first.identity.unit_path.clone()),
                tofu,
            })
        }
        _ => return Err(problem("native_graph_unknown_task_stack")),
    };
    Ok(source)
}

fn insert_named_checks(
    input: &WorkflowBuildInput<'_>,
    sources: &mut Sources,
    ordered: &mut Vec<String>,
) {
    let mut checks = input
        .discovery
        .mise_checks
        .iter()
        .map(|check| check.check.id.as_str())
        .chain(
            input
                .discovery
                .native_checks
                .iter()
                .map(|check| check.id.as_str()),
        )
        .collect::<Vec<_>>();
    checks.sort_unstable();
    for check in checks {
        let id = format!("check-{check}");
        sources.insert(
            id.clone(),
            LogicalJobSource::NamedCheck {
                id: check.to_owned(),
            },
        );
        ordered.push(id);
    }
}

fn insert_gradle(input: &WorkflowBuildInput<'_>, sources: &mut Sources, ordered: &mut Vec<String>) {
    for root in &input.discovery.gradle_roots {
        for project in &root.projects {
            let scope = if root.root.is_empty() {
                String::new()
            } else {
                format!("{}-", velnor_actions_contract::slugify_segment(&root.root))
            };
            let id = format!(
                "{}{}{}",
                velnor_actions_contract::config::gradle::GRADLE_JOB_PREFIX,
                scope,
                velnor_actions_contract::slugify_segment(project.trim_start_matches(':'))
            );
            ordered.push(id.clone());
            sources.insert(
                id,
                LogicalJobSource::Gradle {
                    root: root.root.clone(),
                    project: project.clone(),
                },
            );
        }
    }
}

fn insert_suites(
    input: &WorkflowBuildInput<'_>,
    jobs: &BTreeMap<String, Job>,
    sources: &mut Sources,
    ordered: &mut Vec<String>,
) {
    if let Some(rust) = &input.config.stacks.rust {
        for suite in &rust.suites {
            for (id, phase) in [
                (format!("suite-build-{}", suite.id), SuitePhase::Build),
                (format!("suite-{}", suite.id), SuitePhase::Run),
            ] {
                if jobs.contains_key(&id) {
                    ordered.push(id.clone());
                    sources.insert(
                        id,
                        LogicalJobSource::Suite {
                            id: suite.id.clone(),
                            phase,
                        },
                    );
                }
            }
        }
    }
}

fn insert_workflow_tasks(
    prepared: &PreparedWorkflow,
    jobs: &BTreeMap<String, Job>,
    sources: &mut Sources,
) {
    for task in &prepared.workflow_tasks {
        for id in jobs.keys().filter(|id| task.owns_job_id(id)) {
            use velnor_actions_workflow_renderer::verification_jobs::WorkflowTaskPolicy;
            let kind = match task {
                WorkflowTaskPolicy::Bun(_) => Some(GeneratedWorkflowKind::BunMember),
                WorkflowTaskPolicy::BunSuite(_) => Some(GeneratedWorkflowKind::BunSuite),
                WorkflowTaskPolicy::BunAssets(_) => Some(GeneratedWorkflowKind::BunAssets),
                WorkflowTaskPolicy::Stack(policy) => Some(match policy.kind {
                    velnor_actions_workflow_renderer::verification_jobs::StackJobKind::Dbt => {
                        GeneratedWorkflowKind::Dbt
                    }
                    velnor_actions_workflow_renderer::verification_jobs::StackJobKind::Ansible => {
                        GeneratedWorkflowKind::Ansible
                    }
                }),
                _ => None,
            };
            let source = kind.map_or_else(
                || LogicalJobSource::WorkflowTask {
                    owner: task.id().to_owned(),
                },
                |kind| LogicalJobSource::GeneratedWorkflowTask {
                    owner: task.id().to_owned(),
                    kind,
                },
            );
            sources.insert(id.clone(), source);
        }
    }
}
