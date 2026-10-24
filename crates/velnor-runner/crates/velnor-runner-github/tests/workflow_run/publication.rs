use super::*;

#[test]
fn nonce_publication_is_fast_forward_only_and_checks_exact_reference() -> Result<(), String> {
    let receipt = nonce_receipt()?;
    let responses = [
        (
            200,
            repository(HEAD_REPOSITORY_ID, HEAD_OWNER, HEAD_REPOSITORY, false),
        ),
        (
            200,
            commit_json(NONCE_COMMIT, COMMIT_MESSAGE_WITH_SIGNOFF, TREE, &[OLD_HEAD]),
        ),
        (200, reference_json(&receipt.head_ref, OLD_HEAD)),
        (200, reference_json(&receipt.head_ref, NONCE_COMMIT)),
        (
            200,
            repository(HEAD_REPOSITORY_ID, HEAD_OWNER, HEAD_REPOSITORY, false),
        ),
    ];
    let mut script = Script::responses(&responses);
    let published =
        publish_pull_request_nonce_commit(&mut script, head_target(), &receipt, "token")
            .map_err(|error| error.to_string())?;
    assert_eq!(published.object.sha, NONCE_COMMIT);
    assert_eq!(script.seen.len(), 5);
    let request = script.seen.get(3).ok_or("reference PATCH")?;
    assert_eq!(request.method, Method::Patch);
    assert_eq!(
        request.path,
        "/repos/fork-owner/fork-repo/git/refs/heads/feature/review"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
    assert_eq!(body["sha"], NONCE_COMMIT);
    assert_eq!(body["force"], false);
    Ok(())
}

#[test]
fn nonce_publication_uncertainty_is_not_retried() -> Result<(), String> {
    let receipt = nonce_receipt()?;
    let responses = [
        (
            200,
            repository(HEAD_REPOSITORY_ID, HEAD_OWNER, HEAD_REPOSITORY, false),
        ),
        (
            200,
            commit_json(NONCE_COMMIT, COMMIT_MESSAGE_WITH_SIGNOFF, TREE, &[OLD_HEAD]),
        ),
        (200, reference_json(&receipt.head_ref, OLD_HEAD)),
    ];
    let mut script = Script::with_failure(&responses, TransportFail::Timeout);
    assert_eq!(
        publish_pull_request_nonce_commit(&mut script, head_target(), &receipt, "token"),
        Err(WorkflowRunError::Uncertain)
    );
    assert_eq!(script.seen.len(), 4);
    assert_eq!(script.seen[3].method, Method::Patch);
    Ok(())
}

#[test]
fn malformed_nonce_publication_response_is_uncertain_after_one_patch() -> Result<(), String> {
    let receipt = nonce_receipt()?;
    let responses = [
        (
            200,
            repository(HEAD_REPOSITORY_ID, HEAD_OWNER, HEAD_REPOSITORY, false),
        ),
        (
            200,
            commit_json(NONCE_COMMIT, COMMIT_MESSAGE_WITH_SIGNOFF, TREE, &[OLD_HEAD]),
        ),
        (200, reference_json(&receipt.head_ref, OLD_HEAD)),
    ];
    let mut script = Script::responses(&responses);
    script.replies.push_back(Ok(Exchange {
        status: 200,
        body: b"not json".to_vec(),
    }));
    assert_eq!(
        publish_pull_request_nonce_commit(&mut script, head_target(), &receipt, "token"),
        Err(WorkflowRunError::Uncertain)
    );
    assert_eq!(script.seen.len(), 4);
    assert_eq!(script.seen[3].method, Method::Patch);
    Ok(())
}
