//! Oversized workflow generation fails before replacing in-place output.

use std::fs;

use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};
use velnor_actions_workflow_tree::MAX_WORKFLOW_BYTES;

use crate::impl_common::{TestResult, make_repo};

#[test]
fn workflow_size_failure_preserves_existing_github_tree() -> TestResult {
    let name = "x".repeat(MAX_WORKFLOW_BYTES + 1);
    let config =
        format!("schema = 1\n[workflow]\nname = \"{name}\"\ndefault_branch = \"testmain\"\n");
    let repo = make_repo(&config)?;
    let root = repo.path();
    let old_workflow = root.join(".github/workflows/old.yml");
    fs::create_dir_all(old_workflow.parent().expect("workflow parent"))?;
    fs::write(&old_workflow, b"old workflow\n")?;
    let old_file = root.join(".github/policy.txt");
    fs::write(&old_file, b"old policy\n")?;

    let prep = prepare(root)?;
    let error = generate(&prep, &GenerateOptions::default())
        .expect_err("oversized render must fail before replacement");
    assert!(error.to_string().contains("workflow_too_large"), "{error}");
    assert_eq!(fs::read(old_workflow)?, b"old workflow\n");
    assert_eq!(fs::read(old_file)?, b"old policy\n");
    Ok(())
}
