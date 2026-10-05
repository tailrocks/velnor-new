//! Local entrypoint coverage for `scripts/verify-local.sh`.
//!
//! Full end-to-end execution is prohibitive inside the suite: the
//! `integration` stage re-runs this whole workspace suite (recursing
//! into this very test), and `toolchain` needs `mise` plus network
//! installs. Instead these tests pin the entrypoint's exact step list
//! and fail-closed shape, syntax-check the script by executing
//! `bash -n`, and execute the offline `repo-policy` stage against an
//! isolated freshness fixture. A stage add, remove, or rename fails
//! the inventory test until the expectation moves with it.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::impl_repo_policy::p12_harness as freshness;

/// Workspace root derived from this crate's manifest dir.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Script text for the local verification entrypoint.
fn entrypoint_text() -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(
        workspace_root().join("scripts/verify-local.sh"),
    )?)
}

/// Inline `echo "--- verify-local: <name>"` stages, in file order.
fn inline_stages(script: &str) -> Vec<String> {
    let mut stages = Vec::new();
    for line in script.lines() {
        let Some(name) = line
            .trim()
            .strip_prefix("echo \"--- verify-local: ")
            .and_then(|rest| rest.strip_suffix('"'))
        else {
            continue;
        };
        // The `stage()` helper echoes its `$name` parameter; only
        // literal stage names pin the inventory.
        if name != "$name" {
            stages.push(name.to_owned());
        }
    }
    stages
}

/// Literal first arguments of `stage <name> ...` calls, in file order.
fn staged_calls(script: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let mut stages = Vec::new();
    for line in script.lines() {
        let Some(rest) = line.trim().strip_prefix("stage ") else {
            continue;
        };
        let name = rest
            .split_whitespace()
            .next()
            .ok_or("stage without a name")?;
        stages.push(name.trim_matches('"').to_owned());
    }
    Ok(stages)
}

#[test]
fn verify_local_entrypoint_lists_exact_stages() -> Result<(), Box<dyn Error>> {
    let script = entrypoint_text()?;
    assert_eq!(
        inline_stages(&script),
        ["toolchain", "generated-tree", "fixtures", "integration"],
        "exact inline stage inventory"
    );
    assert_eq!(
        staged_calls(&script)?,
        [
            "fmt",
            "runner-fmt",
            "runner-clippy",
            "runner-nextest",
            "runner-test",
            "runner-doctest",
            "runner-deny",
            "repo-policy",
            "clippy-$safe",
            "test-$safe",
            "doctest-$safe",
            "doc-$safe",
        ],
        "exact staged-call inventory"
    );
    // Fail-closed shape: strict mode, recorded failures, nonzero exit.
    assert!(script.contains("set -uo pipefail"), "strict mode");
    assert!(
        script.contains("FAILURES=\"$FAILURES $1\""),
        "fail() records the stage"
    );
    assert!(script.contains("exit 1"), "failures exit nonzero");
    // The repo-policy stage body is the freshness script at default
    // (offline) flags — the exact command the stage test executes.
    assert!(
        script.contains("stage repo-policy \"${MISE_EXEC[@]}\" bash scripts/check-freshness.sh"),
        "repo-policy stage body"
    );
    // Every locking cargo invocation passes `--locked` (`fmt` takes no
    // such flag; `--version` probes build nothing). The trailing space
    // keeps echo text (`... falls back to cargo test"`) out of the
    // probe: only verb-plus-flags invocations match.
    for line in script.lines() {
        // Doc comments quote commands without flags; only code locks.
        if line.trim_start().starts_with('#') {
            continue;
        }
        let locks = ["cargo clippy ", "cargo test ", "cargo build ", "cargo doc "]
            .iter()
            .any(|probe| line.contains(probe));
        if locks {
            assert!(line.contains("--locked"), "unlocked cargo: {line}");
        }
    }
    assert!(
        script.contains("repo_policy workspace-members"),
        "workspace list uses the private Rust operation"
    );
    assert!(
        script.contains("repo_policy library-members"),
        "library list uses the private Rust operation"
    );
    assert!(
        !script.contains("python3 -c") && !script.contains("python -c"),
        "no inline Python metadata probe remains"
    );
    assert!(
        script.contains("run --workspace --locked"),
        "nextest integration run is locked"
    );
    Ok(())
}

#[test]
fn verify_local_bootstrap_reads_quoted_mise_tool_key() -> Result<(), Box<dyn Error>> {
    let root = workspace_root();
    let output = Command::new("awk")
        .arg("-v")
        .arg("key=aqua:nextest-rs/nextest/cargo-nextest")
        .arg("-f")
        .arg(root.join("scripts/toml-tool-pin.awk"))
        .arg(root.join("mise.toml"))
        .output()?;
    assert!(
        output.status.success(),
        "quoted Aqua key rejected: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout)?.trim(), "0.9.146");
    Ok(())
}

#[test]
fn verify_local_entrypoint_parses() -> Result<(), Box<dyn Error>> {
    let script = workspace_root().join("scripts/verify-local.sh");
    let output = Command::new("bash").arg("-n").arg(&script).output()?;
    assert!(
        output.status.success(),
        "bash -n failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn verify_local_repo_policy_stage_executes() -> Result<(), Box<dyn Error>> {
    let fixture = freshness::passing("verify-local-freshness")?;
    let output = freshness::run_script(&fixture.dir, &[]);
    freshness::cleanup(&fixture);
    freshness::assert_clean(&output?);
    Ok(())
}
