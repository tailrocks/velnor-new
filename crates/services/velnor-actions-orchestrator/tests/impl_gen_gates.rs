//! Generate gate cases: fixed-point, determinism, matrix budget.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, plan_internal, prepare};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, fixture_manifest_json, git, make_repo, snapshot,
};

/// Render `generate` into a fresh preview and read every file's bytes.
fn preview_bytes(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn std::error::Error>> {
    let prep = prepare(root)?;
    let parent = TempDir::new()?;
    let preview_root = parent.path().join("preview");
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview_root.clone()),
        },
    )?;
    let mut out = BTreeMap::new();
    for rel in &report.files_written {
        out.insert(rel.clone(), fs::read(preview_root.join(rel))?);
    }
    // `parent` cleans the preview; the byte map is the assertion surface.
    Ok(out)
}

/// Repository bytes only, ignoring mtimes.
fn content(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn std::error::Error>> {
    Ok(snapshot(root)?
        .into_iter()
        .map(|(rel, (bytes, _))| (rel, bytes))
        .collect())
}

#[test]
fn repeated_generate_is_fixed_point() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    generate(&prep, &GenerateOptions { output_dir: None })?;
    let first = content(root)?;
    let prep = prepare(root)?;
    let report = generate(&prep, &GenerateOptions { output_dir: None })?;
    assert_eq!(
        report.validated_by,
        vec![
            "actionlint@1.7.12".to_owned(),
            "shellcheck@0.11.0".to_owned(),
            "zizmor@1.30.1".to_owned(),
        ],
        "validators recorded: {:?}",
        report.validated_by
    );
    assert_eq!(first, content(root)?, "second generate changes no byte");
    Ok(())
}

#[test]
fn cross_checkout_determinism() -> TestResult {
    let left = make_repo(config_with_branch())?;
    let right = make_repo(config_with_branch())?;
    assert_ne!(
        left.path().canonicalize()?,
        right.path().canonicalize()?,
        "distinct checkouts"
    );
    let left_bytes = preview_bytes(left.path())?;
    let right_bytes = preview_bytes(right.path())?;
    assert_eq!(left_bytes.keys().len(), 5, "five generated files");
    assert_eq!(
        left_bytes, right_bytes,
        "identical inputs at different absolute paths stage identical bytes"
    );
    Ok(())
}

/// Workspace with `members` crates and four task configurations.
fn make_wide_repo(members: u32) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    let configs = ["c1", "c2", "c3", "c4"]
        .iter()
        .map(|name| format!("{{ name = \"{name}\", target = \"host\" }}"))
        .collect::<Vec<_>>()
        .join(", ");
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/config.toml"),
        format!(
            "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\nconfigurations = [{configs}]\n"
        ),
    )?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    let member_list = (0..members)
        .map(|index| format!("\"members/m{index:02}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nmembers = [{member_list}]\n"),
    )?;
    for index in 0..members {
        let member = root.join(format!("members/m{index:02}"));
        fs::create_dir_all(member.join("src"))?;
        fs::write(
            member.join("Cargo.toml"),
            format!("[package]\nname = \"m{index:02}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        fs::write(member.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    Ok(dir)
}

#[test]
fn matrix_budget_enforced_never_truncated() -> TestResult {
    let repo = make_wide_repo(60)?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "wide"], root)?;
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()?;
    let head = String::from_utf8(output.stdout)?.trim().to_owned();
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": serde_json::Value::Null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let err = err_of(plan_internal(&request.to_string()), "budget exceeded")?;
    assert!(
        err.to_string().contains("matrix_budget_exceeded"),
        "budget error, got {err}"
    );
    Ok(())
}
