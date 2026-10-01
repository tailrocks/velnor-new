//! T09 `[stacks.tofu]` roots and dialect evidence cases.
use std::fs;
use tempfile::TempDir;
use velnor_actions_orchestrator::prepare;

use crate::impl_common::{TestResult, git, plan_for};

/// Git-initialized repo with `config` plus extra `files` and `mise.toml`.
fn make_tofu_repo(
    config: &str,
    files: &[(&str, &str)],
    mise: Option<&str>,
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    for (relative, content) in files {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
    }
    if let Some(body) = mise {
        fs::write(root.join("mise.toml"), body)?;
    }
    Ok(dir)
}

/// Minimal config with an explicit branch plus `extra` sections.
fn config_with(extra: &str) -> String {
    format!("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{extra}")
}

#[test]
fn configured_roots_trip_pending_arm_until_t10() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[("main.tf", "")],
        None,
    )?;
    let err = prepare(dir.path()).expect_err("conversion stays pending");
    assert!(err.to_string().contains("tofu_pending_t09"), "{err}");
    Ok(())
}

#[test]
fn configured_root_without_config_errors_naming_the_root() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[("notes.txt", "x\n")],
        None,
    )?;
    let err = prepare(dir.path()).expect_err("empty root fails");
    let text = err.to_string();
    assert!(text.contains("no_effective_config"), "{text}");
    Ok(())
}

#[test]
fn bad_root_spelling_rejected_at_load() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\"../escape\"]\n"),
        &[],
        None,
    )?;
    let err = prepare(dir.path()).expect_err("bad spelling fails");
    assert!(err.to_string().contains("dotdot_segment"), "{err}");
    Ok(())
}

#[test]
fn unknown_tofu_keys_rejected() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\nvars = [\"a\"]\n"),
        &[],
        None,
    )?;
    let err = prepare(dir.path()).expect_err("unknown key fails");
    assert!(err.to_string().contains(".velnor/config.toml"), "{err}");
    Ok(())
}

#[test]
fn missing_roots_rejected() -> TestResult {
    let dir = make_tofu_repo(&config_with("[stacks.tofu]\n"), &[], None)?;
    let err = prepare(dir.path()).expect_err("missing roots fails");
    assert!(err.to_string().contains("missing_required_roots"), "{err}");
    Ok(())
}

#[test]
fn weak_evidence_advises_in_plan() -> TestResult {
    let dir = make_tofu_repo(&config_with(""), &[("main.tf", "")], None)?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(plan.contains("Tofu: not detected (weak evidence"), "{plan}");
    assert!(plan.contains("inferred roots (advisory): [.]"), "{plan}");
    Ok(())
}

#[test]
fn strong_file_evidence_advises_in_plan() -> TestResult {
    let dir = make_tofu_repo(&config_with(""), &[("infra/main.tofu", "")], None)?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains("Tofu: not detected (strong evidence"),
        "{plan}"
    );
    assert!(
        plan.contains("inferred roots (advisory): [infra]"),
        "{plan}"
    );
    Ok(())
}

#[test]
fn mise_opentofu_advises_strong() -> TestResult {
    let dir = make_tofu_repo(
        &config_with(""),
        &[],
        Some("[tools]\nopentofu = \"1.13.1\"\n"),
    )?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains("Tofu: not detected (strong evidence"),
        "{plan}"
    );
    assert!(plan.contains("mise-tool:opentofu"), "{plan}");
    Ok(())
}

#[test]
fn dialect_conflict_errors() -> TestResult {
    let dir = make_tofu_repo(
        &config_with(""),
        &[("main.tofu", "")],
        Some("[tools]\nterraform = \"1.9.0\"\n"),
    )?;
    let err = prepare(dir.path()).expect_err("conflict fails");
    assert!(err.to_string().contains("tofu_dialect_conflict"), "{err}");
    Ok(())
}

#[test]
fn ignore_suppresses_configured_table() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks]\nignore = [\"tofu\"]\n[stacks.tofu]\nroots = [\".\"]\n"),
        &[("main.tf", "")],
        None,
    )?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains("Tofu: ignored (config stacks.ignore)"),
        "{plan}"
    );
    Ok(())
}

#[test]
fn ignore_suppresses_conflict() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks]\nignore = [\"tofu\"]\n"),
        &[("main.tofu", "")],
        Some("[tools]\nterraform = \"1.9.0\"\n"),
    )?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains("Tofu: ignored (config stacks.ignore)"),
        "{plan}"
    );
    Ok(())
}

#[test]
fn terraform_only_repo_stays_silent() -> TestResult {
    let dir = make_tofu_repo(
        &config_with(""),
        &[("main.tf", "")],
        Some("[tools]\nterraform = \"1.9.0\"\n"),
    )?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(!plan.contains("Tofu:"), "{plan}");
    Ok(())
}

#[test]
fn clean_repo_plan_has_no_tofu_line() -> TestResult {
    let dir = make_tofu_repo(&config_with(""), &[("src/lib.rs", "pub fn f() {}\n")], None)?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(!plan.contains("Tofu:"), "{plan}");
    assert!(plan.contains("Rust: none detected"), "{plan}");
    Ok(())
}
