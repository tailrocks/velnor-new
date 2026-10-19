//! A tap-only consumer needs no Rust stack to emit the trusted adapter.
use crate::impl_common::{TestResult, git, make_repo, plan_for};
use std::fs;
use velnor_actions_contract::review_completion::REVIEW_COMPLETION_WORKFLOW_PATH;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
const CONFIG: &str = r#"schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
[workflow.review_completion]
provider = "codex-v1"
check_name = "Review completion"
app_id_variable = "COMPLETION_APP_ID"
app_private_key_secret = "COMPLETION_APP_PRIVATE_KEY"
reviewer_app_id_variable = "REVIEWER_APP_ID"
"#;
#[test]
fn completion_companion_is_native_optional_and_needs_no_rust_stack() -> TestResult {
    let repo = make_repo(CONFIG)?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/homebrew-tap.git",
        ],
        repo.path(),
    )?;
    fs::remove_file(repo.path().join("Cargo.toml"))?;
    fs::remove_dir_all(repo.path().join("src"))?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let text = tree
        .get(REVIEW_COMPLETION_WORKFLOW_PATH)
        .ok_or("completion missing")?;
    for expected in [
        "tailrocks/homebrew-tap",
        "Review completion",
        "branches:\n      - testmain",
        "capture-review-completion-v1",
        "emit-review-completion-v1",
        "timeout-minutes: 5",
    ] {
        assert!(text.contains(expected), "{text}");
    }
    assert!(!text.contains("actions/checkout@"));
    assert!(!text.contains("cargo "));
    assert!(plan_for(&prep)?.contains(REVIEW_COMPLETION_WORKFLOW_PATH));
    fs::write(
        repo.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = 'CI'\ndefault_branch = 'testmain'\n",
    )?;
    assert!(
        render_staged_tree(&prepare(repo.path())?)?
            .get(REVIEW_COMPLETION_WORKFLOW_PATH)
            .is_none()
    );
    Ok(())
}
#[test]
fn completion_companion_filename_cannot_be_preserved() -> TestResult {
    let config = format!("{CONFIG}\n");
    let repo = make_repo(&config.replace(
        "name = \"CI\"",
        "name = \"CI\"\npreserve_workflows = ['review-completion.yml']",
    ))?;
    assert!(prepare(repo.path()).is_err());
    Ok(())
}
