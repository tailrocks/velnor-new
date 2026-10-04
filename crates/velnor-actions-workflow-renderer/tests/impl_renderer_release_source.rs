//! Source snapshot leaf and isolated tool-domain regressions.
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation, StepKind};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;

use super::{fixture, invalid, job, job_steps, with_steps, workflow};

fn source_snapshot_workflow(
    bootstrap_mode: bool,
) -> Result<super::ReleaseWorkflowSpec, RenderError> {
    let mut workflow = workflow(bootstrap_mode, false)?;
    let role = ReleaseRole::SourceSnapshotForge;
    workflow.jobs.clear();
    workflow.helper_registry = fixture::source_helper_registry(bootstrap_mode);
    workflow.jobs.insert(
        role.job_id().to_owned(),
        job(
            role,
            &[],
            None,
            None,
            fixture::source_snapshot(bootstrap_mode)?,
        )?,
    );
    Ok(workflow)
}

#[test]
fn source_snapshot_leaf_uses_readonly_api_and_five_outputs() -> Result<(), RenderError> {
    let workflow = source_snapshot_workflow(false)?;
    let source = workflow
        .jobs
        .get(ReleaseRole::SourceSnapshotForge.job_id())
        .expect("source snapshot job");
    assert_eq!(
        source
            .outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "source-snapshot-artifact-id",
            "source-snapshot-artifact-digest",
            "source-snapshot-blob-sha256",
            "source-commit-sha",
            "source-tree-sha",
        ]
    );
    assert!(source.steps.iter().all(|step| {
        !matches!(&step.kind, StepKind::Action { uses, .. }
            if uses.starts_with("actions/checkout@"))
    }));
    assert!(check_release_jobs(&workflow).is_ok());
    Ok(())
}

#[test]
fn source_snapshot_tool_domains_reject_missing_reordered_or_duplicate() -> Result<(), RenderError> {
    for index in 0..4 {
        let mut missing = source_snapshot_workflow(false)?;
        let mut steps = job_steps(&missing, ReleaseRole::SourceSnapshotForge.job_id())
            .expect("source snapshot steps");
        steps.remove(index);
        missing = with_steps(missing, ReleaseRole::SourceSnapshotForge.job_id(), steps)
            .expect("source snapshot job");
        assert!(invalid(check_release_jobs(&missing)).is_some());
    }

    for (left, right) in [(0, 1), (2, 3)] {
        let mut reordered = source_snapshot_workflow(false)?;
        let mut steps = job_steps(&reordered, ReleaseRole::SourceSnapshotForge.job_id())
            .expect("source snapshot steps");
        steps.swap(left, right);
        reordered = with_steps(reordered, ReleaseRole::SourceSnapshotForge.job_id(), steps)
            .expect("source snapshot job");
        assert!(invalid(check_release_jobs(&reordered)).is_some());
    }

    for (target, source) in [(0, 1), (1, 0), (2, 3), (3, 2)] {
        let mut duplicate = source_snapshot_workflow(false)?;
        let mut steps = job_steps(&duplicate, ReleaseRole::SourceSnapshotForge.job_id())
            .expect("source snapshot steps");
        steps[target] = steps[source].clone();
        duplicate = with_steps(duplicate, ReleaseRole::SourceSnapshotForge.job_id(), steps)
            .expect("source snapshot job");
        assert!(invalid(check_release_jobs(&duplicate)).is_some());
    }
    Ok(())
}

fn helper_for(
    workflow: &super::ReleaseWorkflowSpec,
    operation: SourceBoundOperation,
) -> &CompiledSourceHelper {
    let job = workflow
        .jobs
        .get(ReleaseRole::SourceSnapshotForge.job_id())
        .expect("source snapshot job");
    let invocation = job
        .steps
        .iter()
        .find_map(|step| match &step.kind {
            StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == operation =>
            {
                Some(invocation.clone())
            }
            _ => None,
        })
        .expect("source helper step");
    workflow
        .helper_registry
        .iter()
        .find(|record| record.invocation() == &invocation)
        .expect("source helper record")
}

fn assert_python_only_context(record: &CompiledSourceHelper, planning_key: &str) {
    let recipe = record
        .execution_recipe()
        .expect("qualified execution recipe");
    assert_eq!(
        record.invocation().installed_selectors(),
        &["python@3.14.7".to_owned()]
    );
    assert_eq!(recipe.installed_selectors(), &["python@3.14.7".to_owned()]);
    assert_eq!(record.invocation().execution_prefix(), recipe.prefix());
    assert_eq!(record.environment(), recipe.environment());
    assert_eq!(
        record.environment().get(planning_key),
        Some(&"/owned/planning/bin/gh".to_owned())
    );
    assert_eq!(
        recipe.prefix().first().map(String::as_str),
        Some("/usr/bin/env")
    );
    assert_eq!(recipe.prefix().get(1).map(String::as_str), Some("-i"));
    assert_eq!(recipe.prefix().last().map(String::as_str), Some("--"));
    assert!(recipe.prefix().iter().any(|value| value == "python@3.14.7"));
    assert!(
        !recipe
            .installed_selectors()
            .iter()
            .any(|value| value == "gh@2.102.0")
    );
}

#[test]
fn source_snapshot_and_admission_use_separate_python_only_planning_bindings()
-> Result<(), RenderError> {
    let workflow = source_snapshot_workflow(false)?;
    let snapshot = helper_for(&workflow, SourceBoundOperation::RustReleaseSourceSnapshot);
    let admission = helper_for(
        &workflow,
        SourceBoundOperation::ReleaseAdmissionDefaultBranch,
    );
    assert_python_only_context(snapshot, "VELNOR_SOURCE_SNAPSHOT_GH");
    assert_python_only_context(admission, "VELNOR_ADMISSION_PLANNING_GH");
    assert!(
        snapshot
            .environment()
            .get("VELNOR_ADMISSION_PLANNING_GH")
            .is_none()
    );
    assert!(
        admission
            .environment()
            .get("VELNOR_SOURCE_SNAPSHOT_GH")
            .is_none()
    );
    Ok(())
}
