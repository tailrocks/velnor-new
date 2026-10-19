//! A tap-only consumer needs no Rust stack to emit the trusted adapter.
use crate::impl_common::{TestResult, git, make_repo, plan_for};
use std::fs;
use velnor_actions_contract::review_completion::REVIEW_COMPLETION_WORKFLOW_PATH;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};
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
        "name = \"CI\"\npreserve_workflows = ['.github/workflows/review-completion.yml']",
    ))?;
    let error = prepare(repo.path()).expect_err("native companion cannot be preserved");
    assert!(
        error
            .to_string()
            .contains("cannot_preserve_generator_workflow"),
        "{error}"
    );
    Ok(())
}

#[test]
fn completion_only_schema2_generation_validates_declared_variables() -> TestResult {
    let execution = velnor_actions_contract::ExecutionConfig::hosted_default("ubuntu-26.04")?;
    let config = format!(
        "{}\n{}",
        CONFIG.replace("schema = 1", "schema = 2"),
        toml::to_string(&std::collections::BTreeMap::from([(
            "execution",
            execution
        )]))?
    );
    let repo = make_repo(&config)?;
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
    assert!(prep.config.stacks.rust.is_none());
    assert!(prep.config.distribution.homebrew_tap.is_none());
    let output = tempfile::tempdir()?;
    let destination = output.path().join("generated");
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(destination.clone()),
        },
    )?;
    assert_eq!(report.validated_by.len(), 3);
    let actionlint = fs::read_to_string(destination.join(".github/actionlint.yaml"))?;
    let vars = actionlint
        .split("\n\n")
        .nth(1)
        .ok_or("variable inventory missing")?;
    assert_eq!(
        vars,
        "config-variables:\n  - COMPLETION_APP_ID\n  - REVIEWER_APP_ID"
    );
    assert!(destination.join(REVIEW_COMPLETION_WORKFLOW_PATH).is_file());
    let workflow = destination.join(REVIEW_COMPLETION_WORKFLOW_PATH);
    let original = fs::read_to_string(&workflow)?;
    let mutated = original.replace("vars.COMPLETION_APP_ID", "vars.UNDECLARED_NATIVE_APP");
    assert_ne!(original, mutated);
    fs::write(workflow, mutated)?;
    let args = velnor_actions_actionlint::ActionlintToolchain::staged_lint_argv(
        ".github/actionlint.yaml",
        &[REVIEW_COMPLETION_WORKFLOW_PATH.to_owned()],
    );
    let output = velnor_actions_mise::PinnedToolExec::new(
        vec![
            velnor_actions_mise::PinnedTool::Actionlint,
            velnor_actions_mise::PinnedTool::Shellcheck,
        ],
        std::ffi::OsStr::new("actionlint"),
        args.into_iter().map(Into::into).collect(),
    )?
    .command(&velnor_actions_mise::ToolCatalog::pinned())?
    .with_cwd(destination)
    .run()?;
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.success, "{diagnostic}");
    assert!(
        diagnostic
            .to_ascii_lowercase()
            .contains("undeclared_native_app"),
        "{diagnostic}"
    );
    Ok(())
}
