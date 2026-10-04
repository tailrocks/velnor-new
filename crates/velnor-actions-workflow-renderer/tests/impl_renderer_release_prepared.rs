//! Anonymous prepared-package leaf and source transport regressions.
use std::collections::BTreeMap;

use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::outputs::ActionOutput;
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;
use velnor_actions_workflow_renderer::release_permissions::JobPermissions;
use velnor_actions_workflow_renderer::{DOWNLOAD_ARTIFACT_USES, RenderError};

use super::{fixture, invalid, job, job_steps, with_steps, workflow};

fn prepared_workflow() -> Result<super::ReleaseWorkflowSpec, RenderError> {
    let mut workflow = workflow(false, false)?;
    let role = ReleaseRole::PackagePreparedAnonymous;
    workflow.jobs.clear();
    workflow.helper_registry = fixture::prepared_helper_registry(false);
    workflow.jobs.insert(
        role.job_id().to_owned(),
        job(
            role,
            &[ReleaseRole::SourceSnapshotForge.job_id()],
            None,
            None,
            fixture::prepared_package(false)?,
        )?,
    );
    Ok(workflow)
}

#[test]
fn prepared_leaf_has_exact_source_download_role_and_outputs() -> Result<(), RenderError> {
    let workflow = prepared_workflow()?;
    let role = ReleaseRole::PackagePreparedAnonymous;
    let job = workflow.jobs.get(role.job_id()).expect("prepared job");
    assert_eq!(job.role, role);
    assert_eq!(job.display_name, role.job_id());
    assert_eq!(job.needs, vec![ReleaseRole::SourceSnapshotForge.job_id()]);
    assert!(job.condition.is_none());
    assert!(job.environment.is_none());
    assert_eq!(job.permissions, JobPermissions::expected(role));
    assert_eq!(
        job.outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "package-artifact-id",
            "package-artifact-digest",
            "package-blob-sha256",
        ]
    );
    assert_eq!(job.outputs[2].value.step_id.as_str(), "release-package");
    assert_eq!(
        job.outputs[2].value.output,
        ActionOutput::PreparedBlobSha256
    );
    assert!(job.steps.iter().all(|step| {
        !matches!(&step.kind, StepKind::Action { uses, .. }
            if uses.starts_with("actions/checkout@"))
    }));
    let StepKind::Action {
        uses, with, env, ..
    } = &job.steps[2].kind
    else {
        panic!("source download action")
    };
    assert_eq!(uses, DOWNLOAD_ARTIFACT_USES);
    assert_eq!(
        with,
        &BTreeMap::from([
            (
                "artifact-ids".to_owned(),
                "${{ needs.release-source-snapshot.outputs.source-snapshot-artifact-id }}"
                    .to_owned(),
            ),
            (
                "path".to_owned(),
                "${{ runner.temp }}/velnor/release-source-input".to_owned(),
            ),
        ])
    );
    assert!(env.is_empty());
    assert!(job.steps[2].id.is_none());
    assert!(job.steps[2].condition.is_none());
    assert_eq!(job.steps[2].name, "Download source snapshot");
    let StepKind::Action { with, .. } = &job.steps[4].kind else {
        panic!("prepared upload action")
    };
    assert_eq!(
        with.get("path").map(String::as_str),
        Some("${{ runner.temp }}/velnor/source-intent-prepared/prepared.zip")
    );
    for step in &job.steps {
        if let StepKind::SourceBoundHelper { env, .. } = &step.kind {
            assert!(!env.contains_key("GH_TOKEN"));
            assert!(!env.contains_key("CARGO_REGISTRY_TOKEN"));
            assert!(
                !env.keys()
                    .any(|key| key.starts_with("ACTIONS_ID_TOKEN_REQUEST_"))
            );
        }
    }
    assert!(check_release_jobs(&workflow).is_ok());
    Ok(())
}

#[test]
fn prepared_download_rejects_extra_fields_swapped_source_or_credentials() -> Result<(), RenderError>
{
    let mut extra = prepared_workflow()?;
    let mut steps =
        job_steps(&extra, ReleaseRole::PackagePreparedAnonymous.job_id()).expect("prepared steps");
    let StepKind::Action { with, .. } = &mut steps[2].kind else {
        panic!("source download action")
    };
    with.insert("name".to_owned(), "forged-source".to_owned());
    extra = with_steps(extra, ReleaseRole::PackagePreparedAnonymous.job_id(), steps)
        .expect("prepared job");
    assert!(invalid(check_release_jobs(&extra)).is_some());

    let mut swapped = prepared_workflow()?;
    let mut steps = job_steps(&swapped, ReleaseRole::PackagePreparedAnonymous.job_id())
        .expect("prepared steps");
    let StepKind::Action { with, .. } = &mut steps[2].kind else {
        panic!("source download action")
    };
    with.insert(
        "artifact-ids".to_owned(),
        "${{ needs.release-package.outputs.package-artifact-id }}".to_owned(),
    );
    swapped = with_steps(
        swapped,
        ReleaseRole::PackagePreparedAnonymous.job_id(),
        steps,
    )
    .expect("prepared job");
    assert!(invalid(check_release_jobs(&swapped)).is_some());

    let mut credentialed = prepared_workflow()?;
    let mut steps = job_steps(
        &credentialed,
        ReleaseRole::PackagePreparedAnonymous.job_id(),
    )
    .expect("prepared steps");
    let StepKind::Action { env, .. } = &mut steps[2].kind else {
        panic!("source download action")
    };
    env.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
    credentialed = with_steps(
        credentialed,
        ReleaseRole::PackagePreparedAnonymous.job_id(),
        steps,
    )
    .expect("prepared job");
    assert!(invalid(check_release_jobs(&credentialed)).is_some());
    Ok(())
}

#[test]
fn prepared_download_must_precede_proof_and_exist_once() -> Result<(), RenderError> {
    let mut swapped = prepared_workflow()?;
    let mut steps = job_steps(&swapped, ReleaseRole::PackagePreparedAnonymous.job_id())
        .expect("prepared steps");
    steps.swap(2, 3);
    swapped = with_steps(
        swapped,
        ReleaseRole::PackagePreparedAnonymous.job_id(),
        steps,
    )
    .expect("prepared job");
    assert!(invalid(check_release_jobs(&swapped)).is_some());

    let mut missing = prepared_workflow()?;
    let mut steps = job_steps(&missing, ReleaseRole::PackagePreparedAnonymous.job_id())
        .expect("prepared steps");
    steps.remove(2);
    missing = with_steps(
        missing,
        ReleaseRole::PackagePreparedAnonymous.job_id(),
        steps,
    )
    .expect("prepared job");
    assert!(invalid(check_release_jobs(&missing)).is_some());
    Ok(())
}
