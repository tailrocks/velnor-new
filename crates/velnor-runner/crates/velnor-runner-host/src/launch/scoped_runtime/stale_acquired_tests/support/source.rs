pub(super) fn verified_source() -> velnor_runner_github::workflow_run::VerifiedWorkflowSource {
    use velnor_runner_github::workflow_run::{VerifiedJobWorkflowRef, VerifiedWorkflowSource};

    let full_ref = "owner/repository/.github/workflows/build.yml@refs/heads/main".to_owned();
    VerifiedWorkflowSource {
        repository_id: 101,
        owner_name: "owner".to_owned(),
        repository_name: "repository".to_owned(),
        workflow_run_id: 900,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: full_ref.clone(),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref,
            owner_name: "owner".to_owned(),
            repository_name: "repository".to_owned(),
            workflow_path: ".github/workflows/build.yml".to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: velnor_runner_github::workflow_run::WorkflowRefKind::Branch,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    }
}
