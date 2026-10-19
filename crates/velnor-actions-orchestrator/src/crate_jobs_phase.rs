//! Preserve declared quality and data gates across native execution phases.
//!
//! Same-phase gates remain ordered obligations. A gate in another phase becomes
//! an exact owning-job dependency; it must succeed before this job can start.
//! No-target obligations remain vacuous, as in the ordered crate-job model.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::ProposedTask;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

use crate::OrchestratorError;
use crate::crate_job_ids::job_id_for_member;
use crate::internal::internal;

#[cfg(test)]
#[path = "crate_jobs_phase_workflow_tests.rs"]
mod workflow_tests;

/// Lift runnable external gate owners into deterministic job dependencies.
pub(super) fn external_needs(
    tasks: &[&ProposedTask],
    proposals: &[ProposedTask],
    current_job: &str,
) -> Result<Vec<String>, OrchestratorError> {
    let runnable: BTreeMap<&str, &ProposedTask> = proposals
        .iter()
        .filter(|task| super::is_runnable(task))
        .map(|task| (task.task_id.as_str(), task))
        .collect();
    let mut needs = BTreeSet::from([PLAN_JOB_ID.to_owned()]);
    for task in tasks {
        for gate in task.gated_by.iter().chain(&task.depends_on) {
            let Some(member) = runnable.get(gate.as_str()) else {
                continue;
            };
            let owner = job_id_for_member(proposals, member)?
                .ok_or_else(|| internal("crate_gate_owner_missing"))?;
            if owner != current_job {
                needs.insert(owner);
            }
        }
    }
    Ok(needs.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crate_jobs::crate_jobs_tests::group;
    use velnor_actions_rust::TaskKind;

    #[test]
    fn cross_phase_quality_gate_becomes_exact_owner_dependency() {
        let lint = group("demo", TaskKind::Clippy, &[]);
        let build = group("demo", TaskKind::Build, &[&lint.task_id]);
        let mut test = group("demo", TaskKind::Nextest, &[&lint.task_id]);
        test.depends_on.push(build.task_id.clone());
        let proposals = vec![lint.clone(), build, test.clone()];
        let current = job_id_for_member(&proposals, &test)
            .expect("valid phase")
            .expect("test owner");
        let lint_owner = job_id_for_member(&proposals, &lint)
            .expect("valid phase")
            .expect("lint owner");
        let needs = external_needs(&[&test], &proposals, &current).expect("phase needs");
        assert_eq!(needs, vec![PLAN_JOB_ID.to_owned(), lint_owner]);
        assert!(!needs.contains(&current), "same-phase data stays local");
    }

    #[test]
    fn docs_retain_both_distinct_quality_and_doctest_owners() {
        let lint = group("demo", TaskKind::Clippy, &[]);
        let doctest = group("demo", TaskKind::Doctest, &[&lint.task_id]);
        let mut docs = group("demo", TaskKind::Doc, &[&lint.task_id]);
        docs.depends_on = vec![doctest.task_id.clone(), doctest.task_id.clone()];
        let proposals = vec![docs.clone(), doctest, lint];
        let owner = job_id_for_member(&proposals, &docs)
            .expect("valid phase")
            .expect("docs owner");
        let needs = external_needs(&[&docs], &proposals, &owner).expect("phase needs");
        assert_eq!(needs.len(), 3, "plan, lint and tests, without duplicates");
        assert!(needs.iter().any(|id| id.ends_with("-lint")));
        assert!(needs.iter().any(|id| id.ends_with("-tests")));
        assert!(!needs.contains(&owner));
    }

    #[test]
    fn no_target_gate_does_not_create_a_nonexistent_job_dependency() {
        let mut doctest = group("demo", TaskKind::Doctest, &[]);
        doctest.no_targets = true;
        let docs = group("demo", TaskKind::Doc, &[&doctest.task_id]);
        let proposals = vec![doctest, docs.clone()];
        let owner = job_id_for_member(&proposals, &docs)
            .expect("valid phase")
            .expect("docs owner");
        assert_eq!(
            external_needs(&[&docs], &proposals, &owner).expect("vacuous gate"),
            vec![PLAN_JOB_ID.to_owned()]
        );
    }
}
