//! T09 `[stacks.tofu]` roots and dialect evidence cases.
use std::fs;
use tempfile::TempDir;
use velnor_actions_orchestrator::prepare;

use crate::impl_common::{TestResult, git, install_fixture_release_manifest, plan_for};

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
    install_fixture_release_manifest(root)?;
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
fn configured_roots_convert_to_selected_projects() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[("main.tf", "")],
        None,
    )?;
    let prep = prepare(dir.path())?;
    let selected: Vec<_> = prep
        .discovery
        .statuses
        .iter()
        .filter_map(|status| match status {
            velnor_actions_contract_planning::DetectionStatus::Selected(project) => Some(project),
            velnor_actions_contract_planning::DetectionStatus::Ignored { .. } => None,
        })
        .collect();
    assert_eq!(selected.len(), 1, "one tofu project selected");
    assert_eq!(selected[0].stack_id, "tofu");
    assert_eq!(selected[0].project_root, "");
    let plan = plan_for(&prep)?;
    assert!(plan.contains("Tofu: selected (roots: [.])"), "{plan}");
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

#[test]
fn malformed_root_errors_naming_the_file() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[("main.tf", "variable \"x\" {\n")],
        None,
    )?;
    let err = prepare(dir.path()).expect_err("malformed fails");
    let text = err.to_string();
    assert!(text.contains("malformed_manifest"), "{text}");
    assert!(text.contains("main.tf"), "{text}");
    Ok(())
}

#[test]
fn unknown_block_root_errors() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[("main.tf", "frobnicate \"x\" {}\n")],
        None,
    )?;
    let err = prepare(dir.path()).expect_err("unknown block fails");
    assert!(err.to_string().contains("unknown_block"), "{err}");
    Ok(())
}

#[test]
fn duplicate_declaration_root_errors() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[
            ("a.tf", "variable \"dup\" {}\n"),
            ("b.tf", "variable \"dup\" {}\n"),
        ],
        None,
    )?;
    let err = prepare(dir.path()).expect_err("duplicate fails");
    assert!(err.to_string().contains("duplicate"), "{err}");
    Ok(())
}

#[test]
fn shadowed_garbage_passes_conversion() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[
            ("main.tf", "((( garbage"),
            ("main.tofu", "variable \"x\" {}\n"),
        ],
        None,
    )?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(plan.contains("Tofu: selected (roots: [.])"), "{plan}");
    Ok(())
}

#[test]
fn json_root_converts() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\"]\n"),
        &[("main.tf.json", "{\"variable\": {\"x\": {}}}")],
        None,
    )?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(plan.contains("Tofu: selected (roots: [.])"), "{plan}");
    Ok(())
}

#[test]
fn multiroot_converts_with_sorted_plan_line() -> TestResult {
    let dir = make_tofu_repo(
        &config_with("[stacks.tofu]\nroots = [\".\", \"infra\"]\n"),
        &[
            ("main.tf", "variable \"a\" {}\n"),
            ("infra/main.tf", "variable \"b\" {}\n"),
        ],
        None,
    )?;
    let prep = prepare(dir.path())?;
    let selected = prep
        .discovery
        .statuses
        .iter()
        .filter(|status| {
            matches!(
                status,
                velnor_actions_contract_planning::DetectionStatus::Selected(_)
            )
        })
        .count();
    assert_eq!(selected, 2);
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains("Tofu: selected (roots: [., infra])"),
        "{plan}"
    );
    Ok(())
}

#[test]
fn e4_content_advises_strong_without_table() -> TestResult {
    let dir = make_tofu_repo(
        &config_with(""),
        &[(
            "main.tf",
            "terraform {\n  required_version = \">= 1.6\"\n}\nlocals {\n  cmd = \"terraform plan\"\n}\n",
        )],
        None,
    )?;
    let prep = prepare(dir.path())?;
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains("Tofu: not detected (strong evidence"),
        "{plan}"
    );
    assert!(plan.contains("content:required-version:main.tf"), "{plan}");
    Ok(())
}

#[test]
fn terraform_only_pin_conflicts_with_strong_spelling() -> TestResult {
    let dir = make_tofu_repo(
        &config_with(""),
        &[(
            "main.tofu",
            "terraform {\n  required_version = \"= 1.5.7\"\n}\n",
        )],
        None,
    )?;
    let err = prepare(dir.path()).expect_err("pin conflict fails");
    assert!(err.to_string().contains("tofu_dialect_conflict"), "{err}");
    Ok(())
}
