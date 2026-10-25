use super::*;

#[test]
fn nonce_commit_requires_exact_head_repository_and_uses_single_post() -> Result<(), String> {
    let mut script = Script::responses(&commit_responses());
    let receipt = create_pull_request_nonce_commit(
        &mut script,
        head_target(),
        "feature/review",
        OLD_HEAD,
        NONCE,
        &identity()?,
        "token",
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(receipt.commit.sha, NONCE_COMMIT);
    assert_eq!(receipt.head_repository_id, HEAD_REPOSITORY_ID);
    assert_eq!(script.seen.len(), 4);
    let request = script.seen.get(2).ok_or("commit POST")?;
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.path, "/repos/fork-owner/fork-repo/git/commits");
    let body: serde_json::Value =
        serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
    assert_eq!(body["parents"], serde_json::json!([OLD_HEAD]));
    assert_eq!(body["tree"], TREE);
    assert_eq!(body["author"]["name"], AUTHOR_NAME);
    assert_eq!(body["author"]["email"], AUTHOR_EMAIL);
    assert_eq!(body["committer"]["name"], AUTHOR_NAME);
    assert_eq!(body["committer"]["email"], AUTHOR_EMAIL);
    assert_eq!(body["message"], COMMIT_MESSAGE_WITH_SIGNOFF);
    Ok(())
}

#[test]
fn nonce_commit_timeout_is_uncertain_and_is_not_retried() -> Result<(), String> {
    let mut before = commit_responses();
    before.truncate(2);
    let mut script = Script::with_failure(&before, TransportFail::Timeout);
    assert_eq!(
        create_pull_request_nonce_commit(
            &mut script,
            head_target(),
            "feature/review",
            OLD_HEAD,
            NONCE,
            &identity()?,
            "token"
        ),
        Err(WorkflowRunError::Uncertain)
    );
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[2].method, Method::Post);
    Ok(())
}

#[test]
fn malformed_nonce_commit_response_is_uncertain_after_one_post() -> Result<(), String> {
    let before = [
        (
            200,
            repository(HEAD_REPOSITORY_ID, HEAD_OWNER, HEAD_REPOSITORY, false),
        ),
        (200, commit_json(OLD_HEAD, "Prior head", TREE, &[])),
    ];
    let mut script = Script::responses(&before);
    script.replies.push_back(Ok(Exchange {
        status: 201,
        body: b"not json".to_vec(),
    }));
    assert_eq!(
        create_pull_request_nonce_commit(
            &mut script,
            head_target(),
            "feature/review",
            OLD_HEAD,
            NONCE,
            &identity()?,
            "token"
        ),
        Err(WorkflowRunError::Uncertain)
    );
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[2].method, Method::Post);
    Ok(())
}
