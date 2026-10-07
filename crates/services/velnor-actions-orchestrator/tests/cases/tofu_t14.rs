//! T14 tofu diagnostics: lock/version findings in recommendations plus
//! the read-only bracket over plan and generate.
use std::fs;

use velnor_actions_orchestrator::{GenerateOptions, generate, plan_text_checked, prepare};
use velnor_actions_tofu_core::TofuLockSnapshot;

use crate::support::{TestResult, config_with_branch, make_repo};

/// Fixture config with one tofu root beside the root crate.
fn tofu_config(root: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"{root}\"]\n"
    )
}

/// Fixture config with tofu ignored but configured.
fn ignored_tofu_config(root: &str) -> String {
    format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"tofu\"]\n[stacks.tofu]\nroots = [\"{root}\"]\n"
    )
}

/// Stale lock bytes: pins a provider no config requires.
fn stale_lock() -> &'static str {
    "provider \"example.com/a/b\" {\nversion = \"1.0.0\"\n}\n"
}

/// Corrupt lock bytes: unparseable HCL.
fn corrupt_lock() -> &'static str {
    "provider \"x\" {\n  version===\n"
}

/// True when any recommendation line contains `needle`.
fn has_line(prep: &velnor_actions_orchestrator::GenerationPreparation, needle: &str) -> bool {
    prep.discovery
        .recommendations
        .iter()
        .any(|line| line.contains(needle))
}

#[test]
fn tofu_stale_lock_surfaces_a_recommendation() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), stale_lock())?;
    let prep = prepare(root)?;
    assert!(
        has_line(&prep, "tofu_lockfile_stale"),
        "{:?}",
        prep.discovery.recommendations
    );
    assert!(
        has_line(&prep, "example.com/a/b"),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn tofu_missing_lock_on_provider_root_fails_planning() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "resource \"x\" \"y\" {}\n")?;
    let err = prepare(root).expect_err("provider root without a lock must fail");
    let text = err.to_string();
    assert!(
        text.contains("missing_committed_lock:stacks/a/.terraform.lock.hcl"),
        "names the lock: {text}"
    );
    assert!(
        text.contains("tofu providers lock"),
        "manual remediation rides along: {text}"
    );
    Ok(())
}

#[test]
fn tofu_committed_lock_on_provider_root_plans() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "resource \"x\" \"y\" {}\n")?;
    fs::write(
        root.join("stacks/a/.terraform.lock.hcl"),
        "provider \"example.com/a/b\" {\nversion = \"1.0.0\"\n}\n",
    )?;
    let prep = prepare(root)?;
    assert!(
        !has_line(&prep, "tofu_lockfile_missing"),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn tofu_bare_root_without_lock_stays_silent() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    let prep = prepare(root)?;
    assert!(
        !has_line(&prep, "tofu_lockfile"),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn tofu_corrupt_lock_surfaces_a_recommendation() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), corrupt_lock())?;
    let prep = prepare(root)?;
    assert!(
        has_line(&prep, "tofu_lockfile_corrupt"),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn tofu_version_excluding_mise_pin_surfaces_a_recommendation() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(
        root.join("stacks/a/main.tf"),
        "terraform {\n  required_version = \"= 1.5.7\"\n}\n",
    )?;
    fs::write(root.join("mise.toml"), "[tools]\nopentofu = \"1.13.1\"\n")?;
    let prep = prepare(root)?;
    assert!(
        has_line(&prep, "tofu_required_version_excludes_toolchain"),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn tofu_version_aligned_with_mise_pin_stays_silent() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(
        root.join("stacks/a/main.tf"),
        "terraform {\n  required_version = \">= 1.7.0\"\n}\n",
    )?;
    fs::write(root.join("mise.toml"), "[tools]\nopentofu = \"1.13.1\"\n")?;
    let prep = prepare(root)?;
    assert!(
        !has_line(&prep, "tofu_required_version"),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn tofu_absent_table_means_no_tofu_diagnostics() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    assert!(
        !has_line(&prep, "tofu_"),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn tofu_ignored_stack_means_no_tofu_diagnostics() -> TestResult {
    let repo = make_repo(&ignored_tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), stale_lock())?;
    let prep = prepare(root)?;
    assert!(
        !has_line(&prep, "tofu_"),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn tofu_discovery_keeps_locks_and_workdirs_clean() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), stale_lock())?;
    let roots = ["stacks/a".to_owned()];
    let snap = TofuLockSnapshot::capture(root, &roots);
    let prep = prepare(root)?;
    assert!(snap.verify(&prep.root).is_ok(), "discovery writes nothing");
    assert!(
        !prep.root.join("stacks/a/.terraform").exists(),
        "no init workdir"
    );
    Ok(())
}

#[test]
fn tofu_plan_keeps_locks_and_workdirs_clean() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), stale_lock())?;
    let prep = prepare(root)?;
    let roots = ["stacks/a".to_owned()];
    let snap = TofuLockSnapshot::capture(&prep.root, &roots);
    let _ = plan_text_checked(&prep)?;
    assert!(snap.verify(&prep.root).is_ok(), "plan writes nothing");
    assert!(
        !prep.root.join("stacks/a/.terraform").exists(),
        "no init workdir"
    );
    Ok(())
}

#[test]
fn tofu_generate_keeps_locks_clean() -> TestResult {
    let repo = make_repo(&tofu_config("stacks/a"))?;
    let root = repo.path();
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/a/.terraform.lock.hcl"), stale_lock())?;
    let prep = prepare(root)?;
    let before = fs::read(root.join("stacks/a/.terraform.lock.hcl"))?;
    generate(&prep, &GenerateOptions { output_dir: None })?;
    assert_eq!(
        before,
        fs::read(root.join("stacks/a/.terraform.lock.hcl"))?,
        "generate never touches the lock"
    );
    assert!(
        !root.join("stacks/a/.terraform").exists(),
        "no init workdir"
    );
    Ok(())
}
