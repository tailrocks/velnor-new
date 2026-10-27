// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_contract::{Job, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_orchestrator::{
    GenerationPreparation, finalized_jobs, prepare, render_staged_tree,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::owner_feature_repo;

fn branch_jobs<'jobs>(
    jobs: &'jobs std::collections::BTreeMap<String, Job>,
    base: &str,
) -> Result<Vec<&'jobs Job>, Box<dyn Error>> {
    let read = format!("{base} (read)");
    let write = format!("{base} (write)");
    let matched: Vec<&Job> = jobs
        .values()
        .filter(|job| {
            job.display_name == base || job.display_name == read || job.display_name == write
        })
        .collect();
    if matched.is_empty() {
        return Err(format!("{base} job is missing").into());
    }
    Ok(matched)
}

#[test]
fn repository_orchestrator_tests_prepare_bun_without_changing_the_official_task_record()
-> Result<(), Box<dyn Error>> {
    for sharded in [false, true] {
        let repo = repository_orchestrator_repo(sharded)?;
        let prepared = prepare(repo.path())?;
        let nextest = prepared
            .discovery
            .proposals
            .iter()
            .filter(|task| {
                task.stack_id == "rust"
                    && task.task_kind == "nextest"
                    && task.identity.unit_path == "Cargo.toml"
            })
            .collect::<Vec<_>>();
        assert_ne!(nextest.len(), 0);
        if sharded {
            let partitions = nextest
                .iter()
                .flat_map(|task| task.payload.windows(2))
                .filter(|pair| pair[0] == "--partition")
                .map(|pair| pair[1].to_string_lossy().into_owned())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                partitions,
                BTreeSet::from(["hash:1/2".to_owned(), "hash:2/2".to_owned()])
            );
        } else {
            assert!(
                nextest
                    .iter()
                    .all(|task| { !task.payload.iter().any(|arg| arg == "--partition") })
            );
        }

        assert_orchestrator_execution(&prepared, &nextest)?;
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "the assertion binds preparation order, legacy task selectors, and cache identity"
)]
fn assert_orchestrator_execution(
    prepared: &GenerationPreparation,
    nextest: &[&velnor_actions_contract::ProposedTask],
) -> Result<(), Box<dyn Error>> {
    let jobs = finalized_jobs(prepared)?;
    let workflow = render_staged_tree(prepared)?;
    let document = workflow
        .get(WORKFLOW_PATH)
        .ok_or("generated workflow is missing")?;
    let branch = branch_jobs(&jobs, "Rust / velnor-actions-orchestrator")?;
    let job = branch
        .iter()
        .find(|job| job.display_name.ends_with(" (read)"))
        .or(branch.first())
        .ok_or("orchestrator crate job is missing")?;
    let preparation = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::PreparePinnedTools))
        .ok_or("pinned tool preparation is missing")?;
    let StepKind::Shell { run, .. } = &job.steps[preparation].kind else {
        return Err("pinned tool preparation is not a shell step".into());
    };
    let mut missing = Vec::new();
    if !run.join(" ").contains("bun@${VELNOR_TOOL_BUN}") {
        missing.push("Bun pinned preparation");
    }
    let execution = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match &step.kind {
            StepKind::TaskExecution {
                task_id,
                argv,
                toolchain_inputs,
                ..
            } if nextest.iter().any(|task| task.task_id == *task_id) => {
                Some((index, task_id, argv, toolchain_inputs))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(execution.len(), nextest.len());
    for (index, task_id, argv, tools) in &execution {
        if preparation >= *index {
            missing.push("preparation before execution");
        }
        if tools.tools.iter().any(|tool| tool.starts_with("bun@"))
            || argv.iter().any(|arg| arg.starts_with("bun@"))
        {
            missing.push("Bun must not enter the official TaskExecution record");
        }
        let task = nextest.iter().find(|task| task.task_id == **task_id);
        let Some(task) = task else {
            missing.push("test task metadata is missing");
            continue;
        };
        if let Some(separator) = argv.iter().position(|arg| arg == "--") {
            let selectors = argv[5..separator]
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let expected_argv = BTreeSet::from([
                "rust@${VELNOR_TOOL_RUST}",
                "aqua:nextest-rs/nextest/cargo-nextest@${VELNOR_TOOL_NEXTEST}",
            ]);
            let expected_tools = BTreeSet::from([
                "rust@${VELNOR_TOOL_RUST}",
                "mr-boxington@${VELNOR_TOOL_MR_BOXINGTON}",
                "aqua:nextest-rs/nextest/cargo-nextest@${VELNOR_TOOL_NEXTEST}",
            ]);
            assert_eq!(task.identity.compile_driver, "mbx");
            assert_eq!(task.identity.test_runner, "cargo_nextest");
            assert_eq!(selectors, expected_argv);
            assert_eq!(
                tools
                    .tools
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                expected_tools
            );
            assert!(argv[separator + 1..].starts_with(&[
                "mbx".to_owned(),
                "nextest".to_owned(),
                "run".to_owned(),
            ]));
            assert!(
                argv.ends_with(
                    &task
                        .payload
                        .iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<_>>(),
                )
            );
        } else {
            missing.push("TaskExecution separator is missing");
        }
    }
    if !document.contains("VELNOR_TOOL_BUN_LOCK") {
        missing.push("Bun version and lock cache identity");
    }
    assert!(
        missing.is_empty(),
        "missing tool prerequisites: {missing:?}"
    );
    assert!(document.contains("mise.toml"));
    assert!(document.contains("mise.lock"));
    assert!(document.contains("~/.local/share/mise"));
    assert!(document.contains("${{ runner.temp }}/velnor/cargo"));
    Ok(())
}

#[test]
fn bun_tool_is_limited_to_repository_owned_orchestrator_tests() -> Result<(), Box<dyn Error>> {
    let cases = [
        (
            WorkflowPolicy::VelnorRepositoryV1,
            "velnor-actions-cli",
            false,
        ),
        (WorkflowPolicy::VelnorRepositoryV1, "demo", false),
        (
            WorkflowPolicy::ConsumerV1,
            "velnor-actions-orchestrator",
            false,
        ),
        (WorkflowPolicy::ConsumerV1, "demo", false),
    ];
    for (policy, package, expected_bun) in cases {
        let repo = owner_feature_repo(policy, true, "default", "default")?;
        rename_root_package(repo.path(), package)?;
        write_project_mise(repo.path())?;
        let prepared = prepare(repo.path())?;
        let jobs = finalized_jobs(&prepared)?;
        let target = format!("Rust / {package}");
        let branch = branch_jobs(&jobs, &target)?;
        for job in branch {
            let has_bun = job.steps.iter().any(|step| {
                matches!(
                    &step.kind,
                    StepKind::TaskExecution { argv, toolchain_inputs, .. }
                        if argv.iter().any(|arg| arg.starts_with("bun@"))
                            || toolchain_inputs.tools.iter().any(|tool| tool.starts_with("bun@"))
                )
            });
            assert_eq!(has_bun, expected_bun, "{policy:?} {package}");
            let prepares_bun = job.steps.iter().any(|step| {
                step.role == Some(StepRole::PreparePinnedTools)
                    && matches!(
                        &step.kind,
                        StepKind::Shell { run, .. }
                            if run.join(" ").contains("bun@${VELNOR_TOOL_BUN}")
                    )
            });
            assert_eq!(prepares_bun, expected_bun, "{policy:?} {package}");
        }
    }
    Ok(())
}

fn repository_orchestrator_repo(sharded: bool) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        true,
        "default",
        "default",
    )?;
    rename_root_package(repo.path(), "velnor-actions-orchestrator")?;
    write_project_mise(repo.path())?;
    if sharded {
        let config = repo.path().join(".velnor/config.toml");
        let mut contents = fs::read_to_string(&config)?;
        contents.push_str(
            "\n[test_sharding]\ndefault_shards = 1\n\n[test_sharding.by_manifest]\n\"Cargo.toml\" = 2\n",
        );
        fs::write(config, contents)?;
    }
    Ok(repo)
}

fn rename_root_package(root: &Path, name: &str) -> Result<(), Box<dyn Error>> {
    let manifest = root.join("Cargo.toml");
    let contents = fs::read_to_string(&manifest)?;
    fs::write(
        manifest,
        contents.replacen("name = \"demo\"", &format!("name = \"{name}\""), 1),
    )?;
    Ok(())
}

fn write_project_mise(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        root.join("mise.toml"),
        include_str!("../../../../mise.toml"),
    )?;
    fs::write(
        root.join("mise.lock"),
        include_str!("../../../../mise.lock"),
    )?;
    Ok(())
}

#[path = "bun_prerequisite_cache.rs"]
mod cache;

#[path = "bun_prerequisite_contract.rs"]
mod contract;
