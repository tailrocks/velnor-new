use super::super::build_verification_task_job;
use super::{CHECKOUT, policy};
use velnor_actions_contract_config::{ArtifactBuildOutput, VerificationRunner};
use velnor_actions_contract_workflow::{ARTIFACT_TASK_ID_ENV, StepId, StepKind, StepRole};

#[test]
fn output_task_downloads_plan_and_exports_after_its_single_mise_run() {
    let mut policy = policy("frontend-check", VerificationRunner::LinuxX64);
    policy.task.outputs = vec![ArtifactBuildOutput {
        id: "dist-report".to_owned(),
        path: "dist/frontend-report.json".to_owned(),
        max_bytes: 4096,
    }];
    let job = build_verification_task_job(&policy, CHECKOUT).expect("output task job");

    assert_eq!(job.needs, ["plan"]);
    assert_eq!(job.steps.len(), 7);
    assert_eq!(job.steps[1].name, "Download plan");
    assert_eq!(job.steps[4].name, "Run declared Mise task");
    assert_eq!(job.steps[5].id, Some(StepId::VerificationArtifactExport));
    assert_eq!(
        job.steps[5].role,
        Some(StepRole::VerificationArtifactExport)
    );
    match &job.steps[5].kind {
        StepKind::Internal { operation, env } => {
            assert_eq!(operation, "export-verification-artifact-v1");
            assert_eq!(
                env.get(ARTIFACT_TASK_ID_ENV).map(String::as_str),
                Some("frontend-check")
            );
        }
        other => panic!("expected the typed output exporter, got {other:?}"),
    }

    let runs = job
        .steps
        .iter()
        .filter(|step| match &step.kind {
            StepKind::Shell { run, .. } => run.ends_with(&[
                "mise".to_owned(),
                "run".to_owned(),
                "lint-frontend-check".to_owned(),
            ]),
            _ => false,
        })
        .count();
    assert_eq!(runs, 1);

    match &job.steps[6].kind {
        StepKind::Action { uses, with, .. } => {
            assert!(uses.as_str().starts_with("actions/upload-artifact@"));
            assert_eq!(
                with.get("name").map(String::as_str),
                Some("${{ steps.verification-artifact-export.outputs.artifact_name }}")
            );
            assert_eq!(
                with.get("if-no-files-found").map(String::as_str),
                Some("error")
            );
        }
        other => panic!("expected a pinned artifact upload, got {other:?}"),
    }
}

#[test]
fn macos_verification_tasks_cannot_declare_linux_artifact_outputs() {
    let mut policy = policy("native-format", VerificationRunner::MacosArm64);
    policy.task.outputs = vec![ArtifactBuildOutput {
        id: "format-report".to_owned(),
        path: "dist/format-report.json".to_owned(),
        max_bytes: 4096,
    }];
    let error = build_verification_task_job(&policy, CHECKOUT)
        .expect_err("artifact collection is supported only on Linux x64");
    assert!(
        error
            .to_string()
            .contains("artifact_build_requires_linux_x64")
    );
}
