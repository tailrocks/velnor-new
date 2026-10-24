//! Workflow content checks.

use crate::fixtures::*;
use velnor_runner_github::workflow_run::{WorkflowRunError, get_workflow_content};
use velnor_runner_github::{Method, TransportFail};
#[test]
fn reads_exact_private_workflow_and_checks_authenticated_requests() -> Result<(), String> {
    let mut script = Script::valid();
    let content = get_workflow_content(
        &mut script,
        target(),
        COMMIT,
        ".github/workflows/ci.yml",
        "token-canary",
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(content.repository_id(), REPOSITORY_ID);
    assert_eq!(content.commit_sha(), COMMIT);
    assert_eq!(content.tree_sha(), ROOT_TREE);
    assert_eq!(content.path(), ".github/workflows/ci.yml");
    assert_eq!(content.blob_sha(), BLOB);
    assert_eq!(content.bytes(), CONTENT.as_bytes());
    assert_eq!(content.content_sha256(), CONTENT_SHA256);
    assert_eq!(script.requests.len(), 6);
    assert_eq!(
        script
            .requests
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/repos/ChainArgos/java-monorepo",
            "/repos/ChainArgos/java-monorepo/git/commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "/repos/ChainArgos/java-monorepo/git/trees/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "/repos/ChainArgos/java-monorepo/git/trees/cccccccccccccccccccccccccccccccccccccccc",
            "/repos/ChainArgos/java-monorepo/git/trees/dddddddddddddddddddddddddddddddddddddddd",
            "/repos/ChainArgos/java-monorepo/git/blobs/c87443c43f5b9bcc3b4087b87d53f1785ee3a05e",
        ]
    );
    for request in &script.requests {
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.query, None);
        assert!(
            request
                .headers
                .iter()
                .any(|(key, value)| { key == "Authorization" && value == "Bearer token-canary" })
        );
    }
    Ok(())
}

#[test]
fn rejects_workflow_paths_before_network_access() {
    for path in [
        "ci.yml",
        ".github/../secret.yml",
        ".github/workflows/%2e%2e.yml",
        ".github/workflows/nested/ci.yml",
        ".github/workflows/.yml",
        ".github/workflows/ci.yaml/",
        ".github/workflows/ci.json",
        ".github/workflows/ci\\evil.yml",
    ] {
        let mut script = Script::valid();
        assert_eq!(
            read(&mut script, path),
            Err(WorkflowRunError::InvalidRequest),
            "path: {path}"
        );
        assert_eq!(script.requests.len(), 0);
    }
}

#[test]
fn accepts_supported_workflow_extensions() {
    let mut script = Script::valid();
    script.replace(
        4,
        tree_json(
            WORKFLOWS_TREE,
            &[entry(
                "ci.yaml",
                "100644",
                "blob",
                BLOB,
                Some(CONTENT.len()),
            )],
        ),
    );
    assert_eq!(read(&mut script, ".github/workflows/ci.yaml"), Ok(()));
}

#[test]
fn rejects_repository_scope_and_commit_identity_changes() {
    let mut script = Script::valid();
    script.replace(
        0,
        format!(
            r#"{{"id":{REPOSITORY_ID},"name":"{REPOSITORY}","full_name":"{OWNER}/{REPOSITORY}","private":false,"owner":{{"login":"{OWNER}"}}}}"#
        ),
    );
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::RepositoryMismatch)
    );
    assert_eq!(script.requests.len(), 1);

    let mut script = Script::valid();
    script.replace(
        1,
        commit_json_with_sha("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", ROOT_TREE),
    );
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::Malformed)
    );
    assert_eq!(script.requests.len(), 2);
}

#[test]
fn keeps_transport_and_http_failures_closed() {
    let mut script = Script::valid();
    assert_eq!(read(&mut script, ".github/workflows/ci.yml"), Ok(()));

    for status in [403, 500] {
        let mut script = Script::valid();
        script.replies[0] = Ok(response_status(status, "{}".to_owned()));
        assert_eq!(
            read(&mut script, ".github/workflows/ci.yml"),
            Err(if status >= 500 {
                WorkflowRunError::Uncertain
            } else {
                WorkflowRunError::HttpStatus(status)
            })
        );
    }

    for failure in [TransportFail::Timeout, TransportFail::Reset] {
        let mut script = Script::failure(failure);
        assert_eq!(
            read(&mut script, ".github/workflows/ci.yml"),
            Err(WorkflowRunError::Uncertain)
        );
    }
}
