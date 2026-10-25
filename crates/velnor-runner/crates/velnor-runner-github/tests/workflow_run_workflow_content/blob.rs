//! Workflow content checks.

use crate::fixtures::*;
use velnor_runner_github::workflow_run::{WorkflowRunError, get_workflow_content};

#[test]
fn rejects_blob_identity_encoding_size_and_base64_errors() {
    for blob in [
        blob_json(
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
            "base64",
            9,
            "bmFtZTogY2kK",
        ),
        blob_json(BLOB, "utf-8", 9, "bmFtZTogY2kK"),
        blob_json(BLOB, "base64", 9, "%%%="),
        blob_json(BLOB, "base64", 9, "bmFtZTogY2oK"),
    ] {
        let mut script = Script::valid();
        script.replace(5, blob);
        assert_eq!(
            read(&mut script, ".github/workflows/ci.yml"),
            Err(WorkflowRunError::Malformed)
        );
    }

    let mut script = Script::valid();
    script.replace(5, blob_json(BLOB, "base64", 8, "bmFtZTogY2kK"));
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::Malformed)
    );

    let mut script = Script::valid();
    script.replace(
        4,
        tree_json(
            WORKFLOWS_TREE,
            &[entry("ci.yml", "100644", "blob", BLOB, Some(8))],
        ),
    );
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::Malformed)
    );
}

#[test]
fn accepts_github_base64_line_breaks_but_rejects_decoded_oversize() -> Result<(), String> {
    let mut script = Script::valid();
    script.replace(5, blob_json(BLOB, "base64", 9, "bmFt\nZTogY2kK"));
    let content = get_workflow_content(
        &mut script,
        target(),
        COMMIT,
        ".github/workflows/ci.yml",
        "token",
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(content.bytes(), CONTENT.as_bytes());

    let mut script = Script::valid();
    let large = vec![b'a'; 256 * 1024 + 1];
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &large);
    script.replace(
        4,
        tree_json(
            WORKFLOWS_TREE,
            &[entry("ci.yml", "100644", "blob", BLOB, Some(large.len()))],
        ),
    );
    script.replace(5, blob_json(BLOB, "base64", large.len(), &encoded));
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::ResponseTooLarge)
    );
    Ok(())
}
