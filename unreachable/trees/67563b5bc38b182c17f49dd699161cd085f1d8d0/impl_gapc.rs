//! Gap C (regeneration stability) regression cases.
//!
//! Profile durability, transient-evidence gating, declared-key stickiness,
//! conflict fail-closed, and tool-file preservation across regenerations.

use std::fs;

use velnor_actions_orchestrator::{GenerateOptions, GenerationPreparation, generate, prepare};

use crate::impl_common::{TestResult, config_with_branch, err_of, make_repo, plan_for, snapshot};

/// Finding code when transient-only evidence selects a non-default profile.
const TRANSIENT_CODE: &str = "transient_evidence_requires_declaration";

/// Error code when durable evidence contradicts a declaration.
const CONFLICT_CODE: &str = "profile_conflict";

/// Config with a sticky declared Rust profile and no configurations key.
fn declared_config(driver: &str, runner: &str) -> String {
    format!(
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\ncompile_driver = \"{driver}\"\ntest_runner = \"{runner}\"\n"
    )
}

/// Hand-written workflow invoking `command` (no generator marker).
fn handwritten(command: &str) -> String {
    format!("jobs:\n  build:\n    steps:\n      - run: {command}\n")
}

/// Preview-generate `prep` into a fresh directory; return both trees' bytes.
fn preview_bytes(
    prep: &GenerationPreparation,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let parent = tempfile::TempDir::new()?;
    let preview = parent.path().join("preview");
    generate(
        prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let workflow = fs::read(preview.join(".github/workflows/ci.yml"))?;
    let actionlint = fs::read(preview.join(".github/actionlint.yaml"))?;
    Ok((workflow, actionlint))
}

/// Executable Mise task invoking `command`.
fn write_executable_task(
    root: &std::path::Path,
    command: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root.join(".mise/tasks"))?;
    let task = root.join(".mise/tasks/test");
    fs::write(&task, format!("#!/bin/sh\n{command}\n"))?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&task)?.permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        fs::set_permissions(&task, perms)?;
    }
    Ok(())
}

#[test]
fn transient_mbx_blocks_generate() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::create_dir_all(root.join(".github/workflows"))?;
    fs::write(
        root.join(".github/workflows/ci.yml"),
        handwritten("mbx clippy --package demo"),
    )?;
    let before = snapshot(root)?;
    let prep = prepare(root)?;
    assert_eq!(prep.discovery.workspaces.len(), 1);
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.compile_driver.as_str(), "mbx");
    assert_eq!(workspace.findings.len(), 1);
    assert_eq!(workspace.findings[0].code, TRANSIENT_CODE);
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains(TRANSIENT_CODE),
        "plan reports finding:\n{plan}"
    );
    assert!(
        plan.contains(".github/workflows/ci.yml"),
        "finding names path:\n{plan}"
    );
    let err = err_of(generate(&prep, &GenerateOptions::default()), "generate")?;
    let text = err.to_string();
    assert!(
        text.contains(TRANSIENT_CODE),
        "generate carries code: {text}"
    );
    assert!(
        text.contains("[stacks.rust] compile_driver"),
        "instructs declaration: {text}"
    );
    assert!(
        text.contains("durable executable task outside .github"),
        "instructs durable move: {text}"
    );
    assert_eq!(snapshot(root)?, before, "in-place .github untouched");
    assert!(!root.join(".github/workflows/velnor.yml").exists());
    Ok(())
}

#[test]
fn transient_nextest_blocks_generate() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::create_dir_all(root.join(".github/workflows"))?;
    fs::write(
        root.join(".github/workflows/ci.yml"),
        handwritten("cargo nextest run --package demo"),
    )?;
    let before = snapshot(root)?;
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.test_runner.as_str(), "cargo_nextest");
    assert!(
        workspace
            .findings
            .iter()
            .any(|finding| finding.code == TRANSIENT_CODE),
        "nextest finding recorded"
    );
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains(TRANSIENT_CODE),
        "plan reports finding:\n{plan}"
    );
    let err = err_of(generate(&prep, &GenerateOptions::default()), "generate")?;
    let text = err.to_string();
    assert!(
        text.contains(TRANSIENT_CODE),
        "generate carries code: {text}"
    );
    assert!(
        text.contains("[stacks.rust] test_runner"),
        "instructs declaration: {text}"
    );
    assert_eq!(snapshot(root)?, before, "in-place .github untouched");
    Ok(())
}

#[test]
fn declared_profile_survives_regeneration() -> TestResult {
    let repo = make_repo(&declared_config("mbx", "cargo_nextest"))?;
    let root = repo.path();
    fs::create_dir_all(root.join(".github/workflows"))?;
    fs::write(
        root.join(".github/workflows/ci.yml"),
        handwritten("mbx nextest run --package demo"),
    )?;
    let prep = prepare(root)?;
    assert!(
        prep.discovery.workspaces[0].findings.is_empty(),
        "declaration resolves transient evidence"
    );
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains("mbx compile driver (declared)"),
        "provenance:\n{plan}"
    );
    assert!(
        plan.contains("cargo_nextest test runner (declared)"),
        "provenance:\n{plan}"
    );
    let first = preview_bytes(&prep)?;
    let second = preview_bytes(&prepare(root)?)?;
    assert_eq!(first, second, "generate twice byte-identical");
    let workflow = String::from_utf8(first.0.clone())?;
    assert!(workflow.contains("mbx"), "declared driver rendered");
    let parent = tempfile::TempDir::new()?;
    let report = generate(
        &prepare(root)?,
        &GenerateOptions {
            output_dir: Some(parent.path().join("preview")),
        },
    )?;
    assert_eq!(report.profiles.len(), 1);
    assert_eq!(report.profiles[0].compile_driver, "mbx");
    assert_eq!(report.profiles[0].driver_source, "declared");
    assert_eq!(report.profiles[0].test_runner, "cargo_nextest");
    assert_eq!(report.profiles[0].runner_source, "declared");
    Ok(())
}

#[test]
fn generated_workflow_not_evidence() -> TestResult {
    for marker in [
        "# Generated by Velnor Actions 0.1.0; edit .velnor/config.toml and regenerate.",
        "# Generated by velnor-actions 0.1.0 - DO NOT EDIT",
    ] {
        let repo = make_repo(config_with_branch())?;
        let root = repo.path();
        fs::create_dir_all(root.join(".github/workflows"))?;
        fs::write(
            root.join(".github/workflows/ci.yml"),
            format!(
                "{marker}\nsteps:\n  - run: mbx clippy --package demo\n  - run: cargo nextest run\n"
            ),
        )?;
        let prep = prepare(root)?;
        let workspace = &prep.discovery.workspaces[0];
        assert_eq!(
            workspace.profile.compile_driver.as_str(),
            "cargo",
            "marker ignored: {marker}"
        );
        assert_eq!(workspace.profile.test_runner.as_str(), "cargo_test");
        assert!(
            workspace.profile.evidence.is_empty(),
            "no evidence from generated output"
        );
        assert!(
            workspace.findings.is_empty(),
            "no findings without evidence"
        );
        let plan = plan_for(&prep)?;
        assert!(!plan.contains(TRANSIENT_CODE), "no finding:\n{plan}");
        let first = preview_bytes(&prep)?;
        let second = preview_bytes(&prepare(root)?)?;
        assert_eq!(first, second, "stable across regens for {marker}");
    }
    Ok(())
}

#[test]
fn durable_evidence_stable() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_executable_task(root, "mbx test --package demo")?;
    let before = plan_for(&prepare(root)?)?;
    assert!(before.contains("mbx compile driver (detected)"), "{before}");
    generate(&prepare(root)?, &GenerateOptions::default())?;
    assert!(root.join(".github/workflows/ci.yml").is_file());
    let after = plan_for(&prepare(root)?)?;
    assert_eq!(before, after, "plan identical after in-place regen");
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert!(workspace.findings.is_empty(), "durable needs no finding");
    assert!(
        workspace
            .profile
            .evidence
            .iter()
            .all(|sighting| { sighting.strength.as_str() == "durable" }),
        "all evidence durable"
    );
    Ok(())
}

#[test]
fn profile_conflict_fails_closed() -> TestResult {
    let repo = make_repo(&declared_config("cargo", "cargo_test"))?;
    let root = repo.path();
    fs::create_dir_all(root.join(".github/workflows"))?;
    fs::write(
        root.join(".github/workflows/ci.yml"),
        handwritten("echo ok"),
    )?;
    write_executable_task(root, "mbx test --package demo")?;
    let before = snapshot(root)?;
    let err = err_of(prepare(root).map(|_| ()), "driver conflict")?;
    assert!(err.to_string().contains(CONFLICT_CODE), "conflict: {err}");
    assert_eq!(snapshot(root)?, before, ".github untouched");
    let repo = make_repo(&declared_config("cargo", "cargo_test"))?;
    let root = repo.path();
    write_executable_task(root, "cargo nextest run --package demo")?;
    let err = err_of(prepare(root).map(|_| ()), "runner conflict")?;
    assert!(err.to_string().contains(CONFLICT_CODE), "conflict: {err}");
    Ok(())
}

#[test]
fn tool_files_untouched() -> TestResult {
    let repo = make_repo(&declared_config("mbx", "cargo_nextest"))?;
    let root = repo.path();
    let witnesses = [
        ("mise.toml", "[tools]\n"),
        (".mise.toml", "[tools]\n"),
        ("mise.lock", "{}\n"),
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.90\"\n"),
    ];
    for (rel, body) in witnesses {
        fs::write(root.join(rel), body)?;
    }
    generate(&prepare(root)?, &GenerateOptions::default())?;
    for (rel, body) in witnesses {
        assert_eq!(
            fs::read_to_string(root.join(rel))?,
            body,
            "tool file preserved: {rel}"
        );
    }
    Ok(())
}

#[test]
fn declared_keys_are_typed_and_closed() -> TestResult {
    for config in [
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\ncompile_driver = \"bogus\"\n",
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\ntest_runner = \"bogus\"\n",
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\nshell = \"echo\"\n",
    ] {
        let repo = make_repo(config)?;
        let err = err_of(prepare(repo.path()).map(|_| ()), "bad key rejected")?;
        assert!(!err.to_string().is_empty());
    }
    let repo = make_repo(&declared_config("cargo", "cargo_test"))?;
    let prep = prepare(repo.path())?;
    assert_eq!(
        prep.discovery.workspaces[0].profile.driver_source.as_str(),
        "declared"
    );
    Ok(())
}

#[test]
fn transient_cargo_test_defaults_without_block() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::create_dir_all(root.join(".github/workflows"))?;
    fs::write(
        root.join(".github/workflows/ci.yml"),
        handwritten("cargo test --package demo --locked"),
    )?;
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.test_runner.as_str(), "cargo_test");
    assert!(workspace.findings.is_empty(), "default needs no finding");
    let parent = tempfile::TempDir::new()?;
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(parent.path().join("preview")),
        },
    )?;
    Ok(())
}
