//! Integration tests for `[[stacks.rust.suites]]`, `[workflow.dispatch]`,
//! `preserve_workflows`, and `[agents] claude_md = "symlink"`.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_orchestrator::{
    GenerateOptions, finalized_jobs, generate, prepare, render_staged_tree,
};

use crate::impl_common::{TestResult, err_of, make_repo};

#[test]
fn rust_suites_validate_nextest_profile_package_and_manifest() -> TestResult {
    let config_bad_profile = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
[stacks.rust]
test_runner = "cargo_nextest"
[[stacks.rust.suites]]
id = "quarantine"
profile = "undeclared"
"#;
    let repo = make_repo(config_bad_profile)?;
    fs::create_dir_all(repo.path().join(".config"))?;
    fs::write(
        repo.path().join(".config/nextest.toml"),
        "[profile.ci]\nfail-fast = false\n[profile.quarantine]\nretries = 2\n",
    )?;
    let err = err_of(prepare(repo.path()), "undeclared nextest profile")?;
    assert!(
        err.to_string()
            .contains("unknown_nextest_profile:undeclared"),
        "unexpected error: {err}"
    );

    let config_bad_pkg = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
[stacks.rust]
test_runner = "cargo_nextest"
[[stacks.rust.suites]]
id = "quarantine"
profile = "default"
package = "nonexistent"
"#;
    let repo_pkg = make_repo(config_bad_pkg)?;
    let err = err_of(prepare(repo_pkg.path()), "unknown package")?;
    assert!(
        err.to_string().contains("unknown_package:nonexistent"),
        "unexpected error: {err}"
    );

    let config_bad_manifest = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
[stacks.rust]
test_runner = "cargo_nextest"
[[stacks.rust.suites]]
id = "quarantine"
profile = "default"
manifest_path = "missing/Cargo.toml"
"#;
    let repo_manifest = make_repo(config_bad_manifest)?;
    let err = err_of(prepare(repo_manifest.path()), "unknown manifest")?;
    assert!(
        err.to_string()
            .contains("unknown_manifest_path:missing/Cargo.toml"),
        "unexpected error: {err}"
    );
    Ok(())
}

#[test]
fn rust_suites_render_ci_jobs_and_standalone_suites_workflow() -> TestResult {
    let config = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"

[workflow.dispatch.inputs.test_filter]
type = "string"
description = "Optional nextest filterset"
required = false

[stacks.rust]
test_runner = "cargo_nextest"

[[stacks.rust.suites]]
id = "e2e"
profile = "e2e"
partitions = 2
build = "archive"
required = true
triggers = ["pull_request", "push"]

[[stacks.rust.suites]]
id = "nightly"
profile = "quarantine"
run_ignored = "only"
triggers = ["schedule", "workflow_dispatch"]
schedule = ["0 3 * * *"]
filter_input = "test_filter"
"#;
    let repo = make_repo(config)?;
    fs::create_dir_all(repo.path().join(".config"))?;
    fs::write(
        repo.path().join(".config/nextest.toml"),
        "[profile.ci]\nfail-fast = false\n[profile.e2e]\nfail-fast = false\n[profile.quarantine]\nretries = 2\n",
    )?;
    let prep = prepare(repo.path())?;
    let jobs = finalized_jobs(&prep)?;
    assert!(jobs.contains_key("suite-build-e2e"));
    assert!(jobs.contains_key("suite-e2e"));
    let required = jobs.get("required").ok_or("missing required job")?;
    assert!(required.needs.contains(&"suite-build-e2e".to_owned()));
    assert!(required.needs.contains(&"suite-e2e".to_owned()));

    let tree = render_staged_tree(&prep)?;
    let ci_yaml = tree
        .get(".github/workflows/ci.yml")
        .ok_or("missing ci.yml")?;
    assert!(ci_yaml.contains("suite-build-e2e:"));
    assert!(ci_yaml.contains("suite-e2e:"));
    let suites_yaml = tree
        .get(".github/workflows/suites.yml")
        .ok_or("missing suites.yml")?;
    assert!(suites_yaml.contains("workflow_dispatch:"));
    assert!(suites_yaml.contains("schedule:"));
    assert!(suites_yaml.contains("0 3 * * *"));
    assert!(suites_yaml.contains("suite-nightly:"));
    assert!(suites_yaml.contains("VELNOR_SUITE_FILTERSET"));

    let report = generate(&prep, &GenerateOptions::default())?;
    assert!(
        report
            .files_written
            .contains(&".github/workflows/suites.yml".to_owned())
    );
    assert!(repo.path().join(".github/workflows/suites.yml").is_file());
    Ok(())
}

#[cfg(unix)]
#[test]
fn claude_md_symlink_mode_emits_symlink_and_matches_preview_diff() -> TestResult {
    let config = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
[agents]
claude_md = "symlink"
"#;
    let repo = make_repo(config)?;
    let root = repo.path();
    let prep = prepare(root)?;
    generate(&prep, &GenerateOptions::default())?;

    let claude_path = root.join(".github/CLAUDE.md");
    assert!(fs::symlink_metadata(&claude_path)?.is_symlink());
    assert_eq!(fs::read_link(&claude_path)?, Path::new("AGENTS.md"));
    let ci_yaml = fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    assert!(ci_yaml.contains("test -L .github/CLAUDE.md"));
    assert!(ci_yaml.contains("readlink .github/CLAUDE.md | grep -qxF AGENTS.md"));

    // Second in-place generate succeeds when .github/CLAUDE.md is already a symlink.
    generate(&prep, &GenerateOptions::default())?;
    assert!(fs::symlink_metadata(&claude_path)?.is_symlink());

    let preview_parent = TempDir::new()?;
    let preview = preview_parent.path().join("out");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let preview_claude = preview.join(".github/CLAUDE.md");
    assert!(fs::symlink_metadata(&preview_claude)?.is_symlink());
    assert_eq!(fs::read_link(&preview_claude)?, Path::new("AGENTS.md"));

    let diff = Command::new("diff")
        .args(["-r", "--brief"])
        .arg(preview.join(".github"))
        .arg(root.join(".github"))
        .output()?;
    assert!(
        diff.status.success(),
        "diff failed: {}",
        String::from_utf8_lossy(&diff.stdout)
    );
    Ok(())
}

#[test]
fn preserve_workflows_retains_allowlisted_workflow_across_generate_and_preview() -> TestResult {
    let config = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
preserve_workflows = [".github/workflows/handwritten.yml"]
"#;
    let repo = make_repo(config)?;
    let root = repo.path();
    let workflows = root.join(".github/workflows");
    fs::create_dir_all(&workflows)?;
    let handwritten = b"name: Handwritten\non: workflow_dispatch\njobs:\n  custom:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: cargo test\n";
    fs::write(workflows.join("handwritten.yml"), handwritten)?;
    fs::write(workflows.join("obsolete.yml"), b"name: Obsolete\n")?;

    let prep = prepare(root)?;
    let preview_parent = TempDir::new()?;
    let preview = preview_parent.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    generate(&prep, &GenerateOptions::default())?;

    for target in [&preview, root] {
        let wf_dir = target.join(".github/workflows");
        assert_eq!(fs::read(wf_dir.join("handwritten.yml"))?, handwritten);
        assert!(!wf_dir.join("obsolete.yml").exists());
        assert!(wf_dir.join("ci.yml").is_file());
    }
    Ok(())
}
