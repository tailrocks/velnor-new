//! T11 module-boundary cases over `prepare`: errors fail, findings pass.
use std::fs;
use tempfile::TempDir;
use velnor_actions_orchestrator::prepare;

use crate::impl_common::{TestResult, git, plan_for};

/// Git-initialized repo with `config` plus extra `files`.
fn make_tofu_repo(
    config: &str,
    files: &[(&str, &str)],
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
    Ok(dir)
}

/// Minimal config with an explicit branch plus `extra` sections.
fn config_with(extra: &str) -> String {
    format!("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{extra}")
}

#[test]
fn missing_module_target_fails_prepare() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[("main.tf", "module \"a\" {\n  source = \"./mods/a\"\n}\n")],
    )?;
    let err = prepare(dir.path()).expect_err("missing target fails");
    assert!(err.to_string().contains("missing_target"), "{err}");
    Ok(())
}

#[test]
fn module_cycle_fails_prepare() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[
            ("main.tf", "variable \"root\" {}\n"),
            ("a/main.tf", "module \"b\" {\n  source = \"../b\"\n}\n"),
            ("b/main.tf", "module \"a\" {\n  source = \"../a\"\n}\n"),
        ],
    )?;
    let err = prepare(dir.path()).expect_err("cycle fails");
    assert!(err.to_string().contains("module_cycle"), "{err}");
    Ok(())
}

#[test]
fn escaping_module_source_fails_prepare() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[
            ("main.tf", "variable \"root\" {}\n"),
            ("sub/main.tf", "module \"o\" {\n  source = \"../../o\"\n}\n"),
        ],
    )?;
    let err = prepare(dir.path()).expect_err("escape fails");
    assert!(err.to_string().contains("module_escape"), "{err}");
    Ok(())
}

#[test]
fn diamond_and_remote_convert() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[
            (
                "main.tf",
                "module \"a\" {\n  source = \"./mods/a\"\n}\n\
                 module \"reg\" {\n  source = \"ns/name/sys\"\n}\n",
            ),
            ("mods/a/main.tf", "variable \"x\" {}\n"),
        ],
    )?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(plan.contains("Tofu: selected (roots: [.])"), "{plan}");
    Ok(())
}
