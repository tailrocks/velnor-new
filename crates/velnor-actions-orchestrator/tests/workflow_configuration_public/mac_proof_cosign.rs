// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::{StepKind, StepRole, WorkflowPolicy};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::crane_prerequisite::{rename_root_package, write_project_mise};
use super::maintenance_features::owner_feature_repo;

#[test]
fn mac_proof_tool_is_ready_only_for_its_repository_test_owner() -> Result<(), Box<dyn Error>> {
    for (policy, package, needed) in [
        (
            WorkflowPolicy::VelnorRepositoryV1,
            "velnor-actions-orchestrator",
            true,
        ),
        (
            WorkflowPolicy::ConsumerV1,
            "velnor-actions-orchestrator",
            false,
        ),
        (
            WorkflowPolicy::VelnorRepositoryV1,
            "velnor-actions-cli",
            false,
        ),
        (WorkflowPolicy::VelnorRepositoryV1, "demo", false),
    ] {
        let repo = owner_feature_repo(policy, true, "default", "default")?;
        rename_root_package(repo.path(), package)?;
        write_project_mise(repo.path())?;
        let prepared = prepare(repo.path())?;
        let nextest = prepared
            .discovery
            .proposals
            .iter()
            .filter(|task| task.task_kind == "nextest" && task.display_name == package)
            .map(|task| task.task_id.as_str())
            .collect::<BTreeSet<_>>();
        assert!(!nextest.is_empty());
        let jobs = finalized_jobs(&prepared)?;
        let mut preparations = 0;
        for job in jobs.values() {
            let matching = job
                .steps
                .iter()
                .enumerate()
                .filter(|(_, step)| step.name == "Prepare pinned Mac proof verifier")
                .collect::<Vec<_>>();
            if job.display_name != format!("Rust / {package}") || !needed {
                assert!(
                    matching.is_empty(),
                    "unrelated job received Cosign: {}",
                    job.display_name
                );
                continue;
            }
            assert_eq!(matching.len(), 1);
            let (index, step) = matching[0];
            assert_eq!(step.role, Some(StepRole::PrepareRuntimeTools));
            let executions = job
                .steps
                .iter()
                .enumerate()
                .filter_map(|(position, step)| match &step.kind {
                    StepKind::TaskExecution { task_id, .. }
                        if nextest.contains(task_id.as_str()) =>
                    {
                        Some(position)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(!executions.is_empty());
            assert!(executions.iter().all(|position| index < *position));
            preparations += 1;
        }
        assert_eq!(preparations, usize::from(needed));
    }
    Ok(())
}
