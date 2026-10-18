use super::*;

#[test]
fn rejects_run_source_and_base_graph_from_a_different_context() -> Result<(), String> {
    let (mut run, history, mut api) = context()?;
    run.pull_requests[0].number += 1;
    assert!(matches!(
        verify(
            &run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::WorkflowSource
        ))
    ));

    let (run, history, mut api) = context()?;
    api.commits.insert(
        CURRENT_BASE.to_owned(),
        commit_json(CURRENT_BASE, "rewritten base", &[]),
    );
    assert!(matches!(
        verify(
            &run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::BaseAncestry
        ))
    ));

    let (mut old_run, history, mut api) = context()?;
    old_run.head_sha = PRIOR_HEAD.to_owned();
    api.commits.insert(
        PRIOR_HEAD.to_owned(),
        commit_json(PRIOR_HEAD, "Pre-retarget workflow", &[ACK_BASE]),
    );
    assert!(matches!(
        verify(
            &old_run,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::RunCommit
        ))
    ));

    let (mut wrong_repo, history, mut api) = context()?;
    wrong_repo.repository.id += 1;
    assert!(matches!(
        verify(
            &wrong_repo,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::WorkflowSource
        ))
    ));

    let (mut wrong_fork, history, mut api) = context()?;
    if let Some(head_repository) = wrong_fork.head_repository.as_mut() {
        head_repository.id += 1;
    }
    assert!(matches!(
        verify(
            &wrong_fork,
            &history,
            &mut api,
            &acknowledgement(),
            &publication()?
        ),
        Err(PullRequestExecutionError::Proof(
            PullRequestExecutionMismatch::WorkflowSource
        ))
    ));
    Ok(())
}

#[test]
fn commit_lookup_from_execution_uses_exact_repository_and_rejects_absent_objects() {
    let mut api = CommitApi::default();
    assert_eq!(
        get_git_commit(&mut api, OWNER, REPOSITORY, NONCE_HEAD, "token"),
        Err(WorkflowRunError::HttpStatus(404))
    );
}

#[test]
fn direct_head_run_requires_the_same_nonce_publication() -> Result<(), String> {
    let (mut run, history, mut api) = context()?;
    run.head_sha = NONCE_HEAD.to_owned();
    let source = source(&run)?;
    let proof = verify_pull_request_execution(
        &mut api,
        &run,
        &source,
        PullRequestExecutionContext {
            expected: RepositoryTarget {
                id: REPOSITORY_ID,
                owner: OWNER,
                name: REPOSITORY,
            },
            expected_head_repository_id: HEAD_REPOSITORY_ID,
            expected_head_ref: "feature",
            expected_base_ref: "sea/integration",
            history: &history,
            acknowledgement: &acknowledgement(),
            publication: &publication()?,
        },
        "token",
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(proof.run_base_sha(), None);
    Ok(())
}
