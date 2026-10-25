//! Workflow content checks.

use crate::fixtures::*;
use velnor_runner_github::workflow_run::WorkflowRunError;
#[test]
fn rejects_duplicate_truncated_wrong_tree_and_missing_entries() {
    let mut script = Script::valid();
    script.replace(
        2,
        tree_json(
            ROOT_TREE,
            &[
                entry(".github", "040000", "tree", GITHUB_TREE, None),
                entry(".github", "040000", "tree", GITHUB_TREE, None),
            ],
        ),
    );
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::Malformed)
    );

    let mut script = Script::valid();
    script.replace(
        2,
        format!(
            r#"{{"sha":"{ROOT_TREE}","tree":[{}],"truncated":true}}"#,
            entry(".github", "040000", "tree", GITHUB_TREE, None)
        ),
    );
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::Malformed)
    );

    let mut script = Script::valid();
    script.replace(
        2,
        tree_json("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", &[]),
    );
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::Malformed)
    );

    let mut script = Script::valid();
    script.replace(2, tree_json(ROOT_TREE, &[]));
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::Malformed)
    );

    let mut script = Script::valid();
    script.replace(2, format!("{}{{}}", tree_json(ROOT_TREE, &[])));
    assert_eq!(
        read(&mut script, ".github/workflows/ci.yml"),
        Err(WorkflowRunError::Malformed)
    );
}

#[test]
fn rejects_directory_and_file_modes_that_are_not_regular_tree_objects() {
    for (mode, kind) in [("100644", "blob"), ("120000", "blob"), ("160000", "commit")] {
        let mut script = Script::valid();
        script.replace(
            2,
            tree_json(
                ROOT_TREE,
                &[entry(".github", mode, kind, GITHUB_TREE, None)],
            ),
        );
        assert_eq!(
            read(&mut script, ".github/workflows/ci.yml"),
            Err(WorkflowRunError::Malformed)
        );
    }

    for (mode, kind) in [("120000", "blob"), ("160000", "commit"), ("040000", "tree")] {
        let mut script = Script::valid();
        script.replace(
            4,
            tree_json(
                WORKFLOWS_TREE,
                &[entry("ci.yml", mode, kind, BLOB, Some(CONTENT.len()))],
            ),
        );
        assert_eq!(
            read(&mut script, ".github/workflows/ci.yml"),
            Err(WorkflowRunError::Malformed)
        );
    }
}

#[test]
fn accepts_regular_and_executable_workflow_blobs() {
    for mode in ["100644", "100755"] {
        let mut script = Script::valid();
        script.replace(
            4,
            tree_json(
                WORKFLOWS_TREE,
                &[entry("ci.yml", mode, "blob", BLOB, Some(CONTENT.len()))],
            ),
        );
        assert_eq!(read(&mut script, ".github/workflows/ci.yml"), Ok(()));
    }
}
