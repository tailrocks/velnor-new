//! Lockless Cargo metadata discovery must stay dependency-free and write-free.

use std::fs;

use crate::impl_common::{TestResult, config_with_branch, make_repo, snapshot};
use velnor_actions_orchestrator::prepare;

#[test]
fn prepare_lockless_with_deps_is_locked_no_deps_and_writes_nothing() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1\"\nvelnor-poison-probe-nonexistent = \"1\"\n",
    )?;
    let before = snapshot(root)?;
    let prep = prepare(root)?;
    assert_eq!(before, snapshot(root)?, "prepare must not write");
    assert!(!root.join("Cargo.lock").exists(), "no lockfile synthesized");
    assert!(!prep.discovery.workspaces.is_empty(), "demo detected");
    let argv = velnor_actions_mise::MetadataDiscovery::new(root.join("Cargo.toml"))?.cargo_argv();
    let skips = argv.iter().any(|arg| arg == "--no-deps");
    assert!(skips, "discovery skips resolution");
    assert!(argv.iter().any(|arg| arg == "--locked"));
    let probe = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .env("CARGO_HTTP_PROXY", "http://127.0.0.1:9/")
        .env("CARGO_HTTPS_PROXY", "http://127.0.0.1:9/")
        .output()?;
    assert!(probe.status.success(), "no-deps never fetches");
    assert!(probe.stderr.is_empty(), "no index chatter");
    assert_eq!(before, snapshot(root)?, "locked no-deps must not write");
    assert!(!root.join("Cargo.lock").exists(), "no lockfile synthesized");
    Ok(())
}
