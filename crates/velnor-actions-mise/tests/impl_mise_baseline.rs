//! Exact-base baseline lookup cases (PAR-5.10).
use std::ffi::OsString;
use velnor_actions_mise::{BaselineLookup, MiseError, ToolCatalog};

const BASE: &str = "0123456789abcdef0123456789abcdef01234567";

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn lookup() -> Result<BaselineLookup, String> {
    BaselineLookup::new(BASE, "ci.yml", "main", "coverage-manifests").map_err(|err| err.to_string())
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn baseline_lookup_records_exact_inputs() -> Result<(), String> {
    let lookup = lookup()?;
    assert_eq!(lookup.base_sha(), BASE);
    assert_eq!(lookup.workflow(), "ci.yml");
    assert_eq!(lookup.branch(), "main");
    assert_eq!(lookup.artifact(), "coverage-manifests");
    Ok(())
}

#[test]
fn baseline_list_args_are_fixed_and_exact() -> Result<(), String> {
    assert_eq!(
        lookup()?.list_args(),
        strings(&[
            "run",
            "list",
            "--workflow",
            "ci.yml",
            "--branch",
            "main",
            "--json",
            "databaseId,headSha,event,conclusion,headBranch",
            "--limit",
            "50",
        ])
    );
    Ok(())
}

#[test]
fn baseline_argv_runs_pinned_gh() -> Result<(), String> {
    let catalog = pinned();
    let lookup = lookup()?;
    assert_eq!(
        lookup.list_argv(&catalog).map_err(|err| err.to_string())?,
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "gh@2.102.0",
            "--",
            "gh",
            "run",
            "list",
            "--workflow",
            "ci.yml",
            "--branch",
            "main",
            "--json",
            "databaseId,headSha,event,conclusion,headBranch",
            "--limit",
            "50",
        ])
    );
    let command = lookup
        .command(&catalog, lookup.list_args())
        .map_err(|err| err.to_string())?;
    assert_eq!(
        command.argv(),
        lookup.list_argv(&catalog).map_err(|err| err.to_string())?
    );
    Ok(())
}

#[test]
fn baseline_lookup_commands_carry_baseline_policy() -> Result<(), String> {
    let catalog = pinned();
    let lookup = lookup()?;
    let command = lookup
        .command(&catalog, lookup.list_args())
        .map_err(|err| err.to_string())?;
    let debug = format!("{command:?}");
    assert!(
        debug.contains("Baseline"),
        "baseline lookup must carry Baseline: {debug}"
    );
    Ok(())
}

#[test]
fn baseline_lookup_rejects_malformed_inputs() {
    for (base, workflow, branch, artifact) in [
        ("abc123", "ci.yml", "main", "coverage-manifests"),
        (
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            "ci.yml",
            "main",
            "coverage-manifests",
        ),
        (BASE, "", "main", "coverage-manifests"),
        (
            BASE,
            "https://example.com/w.yml",
            "main",
            "coverage-manifests",
        ),
        (BASE, "velnor*.yml", "main", "coverage-manifests"),
        (BASE, "ci.yml", "main;evil", "coverage-manifests"),
        (BASE, "ci.yml", "$BRANCH", "coverage-manifests"),
        (BASE, "ci.yml", "-main", "coverage-manifests"),
        (BASE, "ci.yml", "main:evil", "coverage-manifests"),
        (BASE, "ci.yml", "main\non: [push]", "coverage-manifests"),
        (BASE, "ci.yml", "main", ""),
        (BASE, "ci.yml", "main", "coverage/*"),
        (BASE, "ci.yml", "main", "two names"),
        (BASE, "ci.yml", "main", "a/b"),
    ] {
        let err = BaselineLookup::new(base, workflow, branch, artifact)
            .expect_err("malformed lookup input must fail");
        assert!(
            matches!(err, MiseError::InvalidBaselineInput { .. }),
            "typed rejection: {err}"
        );
    }
}
