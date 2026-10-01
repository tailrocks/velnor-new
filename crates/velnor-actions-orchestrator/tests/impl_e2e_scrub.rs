//! End-to-end credential-scrub case over tempdir fixtures.
//!
//! Runs the real `prepare` → `render_staged_tree` path and asserts the
//! emitted workflow text scrubs ambient auth from repo-code steps.

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use crate::impl_common::{TestResult, config_with_branch, make_repo};

#[test]
fn emitted_yaml_scrubs_repo_code_steps() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or("missing workflow in staged tree")?;
    // Obligation wrappers execute repository code: the unset prelude
    // precedes the payload, and the scrub overlay blanks inheritance.
    assert!(
        yaml.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "wrapper must unset:\n{yaml}"
    );
    assert!(
        yaml.contains("GITHUB_TOKEN: \"\""),
        "steps must scrub:\n{yaml}"
    );
    Ok(())
}
