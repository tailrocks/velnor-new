use super::*;

#[test]
fn historical_pull_request_source_ignores_mutable_nested_refs_but_keeps_identity_checks()
-> Result<(), &'static str> {
    let mut run: WorkflowRun = serde_json::from_str(&pull_request_run_json()).map_err(|_| "run")?;
    let source = CompleteSourceMetadata {
        owner_name: OWNER,
        repository_name: REPOSITORY,
        job_workflow_ref: "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/pull/2110/merge",
        workflow_run_id: RUN_ID,
        event_name: "pull_request",
    };

    run.head_sha = "dddddddddddddddddddddddddddddddddddddddd".to_owned();
    let pull = run.pull_requests.first_mut().ok_or("pull request")?;
    pull.head.reference = "updated-feature-name".to_owned();
    pull.head.sha = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_owned();
    pull.base.reference = "updated-base-name".to_owned();
    pull.base.sha = "ffffffffffffffffffffffffffffffffffffffff".to_owned();
    assert_eq!(
        verify_workflow_source(source, &run, target())
            .map_err(|_| "historical pull request source")?
            .head_sha,
        run.head_sha
    );
    let association =
        verify_pull_request_run_association(&run, 2110, REPOSITORY_ID, HEAD_REPOSITORY_ID)
            .map_err(|_| "historical pull request association")?;
    assert_eq!(association.run_head_sha, run.head_sha);
    assert_eq!(association.head_repository_id, HEAD_REPOSITORY_ID);
    assert_eq!(association.base_repository_id, REPOSITORY_ID);

    let mut wrong_fork = run.clone();
    let pull = wrong_fork.pull_requests.first_mut().ok_or("pull request")?;
    let head_repository = pull.head.repository.as_mut().ok_or("head repository")?;
    head_repository.id += 1;
    assert_eq!(
        verify_workflow_source(source, &wrong_fork, target()),
        Err(SourceMismatch::WorkflowRef)
    );
    assert_eq!(
        verify_pull_request_run_association(&wrong_fork, 2110, REPOSITORY_ID, HEAD_REPOSITORY_ID,),
        Err(PullRequestMismatch::HeadRepository)
    );

    let mut wrong_base = run.clone();
    let pull = wrong_base.pull_requests.first_mut().ok_or("pull request")?;
    let base_repository = pull.base.repository.as_mut().ok_or("base repository")?;
    base_repository.id += 1;
    assert_eq!(
        verify_workflow_source(source, &wrong_base, target()),
        Err(SourceMismatch::WorkflowRef)
    );

    let mut duplicate = run.clone();
    let pull = duplicate
        .pull_requests
        .first()
        .ok_or("pull request")?
        .clone();
    duplicate.pull_requests.push(pull);
    assert_eq!(
        verify_workflow_source(source, &duplicate, target()),
        Err(SourceMismatch::WorkflowRef)
    );
    assert_eq!(
        verify_pull_request_run_association(&duplicate, 2110, REPOSITORY_ID, HEAD_REPOSITORY_ID,),
        Err(PullRequestMismatch::Number)
    );
    assert_eq!(
        verify_pull_request_run_association(&run, 2111, REPOSITORY_ID, HEAD_REPOSITORY_ID,),
        Err(PullRequestMismatch::Number)
    );
    let mut wrong_repository = run.clone();
    wrong_repository.repository.id += 1;
    assert_eq!(
        verify_pull_request_run_association(
            &wrong_repository,
            2110,
            REPOSITORY_ID,
            HEAD_REPOSITORY_ID,
        ),
        Err(PullRequestMismatch::BaseRepository)
    );
    Ok(())
}

#[test]
fn pull_request_request_rejects_invalid_number_before_transport() {
    let mut script = Script::response("{}");
    assert_eq!(
        get_pull_request(&mut script, OWNER, REPOSITORY, 0, "token-canary"),
        Err(WorkflowRunError::InvalidRequest)
    );
    assert_eq!(script.seen, Vec::<SessionRequest>::new());
}

#[test]
fn pull_request_request_uses_the_exact_repository_path() -> Result<(), &'static str> {
    let mut script = Script::response(&pull_request_json());
    let pull = get_pull_request(&mut script, OWNER, REPOSITORY, 2110, "token-canary")
        .map_err(|_| "pull request")?;
    assert_eq!(pull.number, 2110);
    assert_eq!(pull.base.reference, "sea/integration");
    assert_eq!(
        pull.head.repository.as_ref().map(|repo| repo.id),
        Some(HEAD_REPOSITORY_ID)
    );
    assert_eq!(
        script.seen.first().map(|request| request.path.as_str()),
        Some("/repos/ChainArgos/java-monorepo/pulls/2110")
    );
    Ok(())
}

#[test]
fn api_failures_do_not_create_source_records() {
    let mut script = Script::failure(TransportFail::Timeout);
    assert_eq!(
        get_workflow_run(&mut script, OWNER, REPOSITORY, RUN_ID, "token-canary"),
        Err(WorkflowRunError::Uncertain)
    );
    let mut script = Script {
        reply: Ok(Exchange {
            status: 403,
            body: Vec::new(),
        }),
        seen: Vec::new(),
    };
    assert_eq!(
        get_workflow_run(&mut script, OWNER, REPOSITORY, RUN_ID, "token-canary"),
        Err(WorkflowRunError::HttpStatus(403))
    );
}

#[test]
fn invalid_api_path_and_token_do_not_reach_transport() {
    let mut script = Script::response("{}");
    assert_eq!(
        get_workflow_run(&mut script, "owner/repo", REPOSITORY, RUN_ID, "token"),
        Err(WorkflowRunError::InvalidRequest)
    );
    assert_eq!(
        get_workflow_run(&mut script, OWNER, REPOSITORY, RUN_ID, "bad\ntoken"),
        Err(WorkflowRunError::InvalidRequest)
    );
    assert_eq!(script.seen, Vec::<SessionRequest>::new());
}
