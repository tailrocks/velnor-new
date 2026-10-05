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
#[path = "../examples/verify_local_metadata.rs"]
mod verify_local_metadata;

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
    // The repo-policy stage runs the freshness script under the same
    // explicit pinned toolchain used by every other Cargo invocation.
    assert!(
        script.contains("stage repo-policy \"${MISE_EXEC[@]}\" scripts/check-freshness.sh"),
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
        let locks = [
            "cargo clippy ",
            "cargo test ",
            "cargo build ",
            "cargo doc ",
            "cargo run ",
        ]
        .iter()
        .any(|probe| line.contains(probe));
        if locks {
            assert!(line.contains("--locked"), "unlocked cargo: {line}");
        }
    }
    assert!(
        !script.contains("python"),
        "no Python dependency or inline code"
    );
    assert!(
        script.contains("mise config get --file mise.toml")
            && script.contains("mise config get --file .velnor/version-policy.toml"),
        "toolchain pins use Mise's native TOML reader"
    );
    assert_eq!(
        script
            .matches("cargo metadata --locked --no-deps --format-version 1 --offline")
            .count(),
        1,
        "metadata helper keeps one locked, offline Cargo probe definition"
    );
    assert!(
        script.contains("cargo run --locked --offline --quiet -p velnor-actions-cli")
            && script.contains("--example verify_local_metadata"),
        "metadata JSON is read by the locked, offline native helper"
    );
    assert!(
        script.contains("cargo_package_names packages")
            && script.contains("cargo_package_names libraries"),
        "both package and library lists are read"
    );
    assert!(
        script.contains("run --workspace --locked"),
        "nextest integration run is locked"
    );
    Ok(())
}

#[test]
fn verify_local_metadata_selects_sorted_packages_and_libraries() -> Result<(), Box<dyn Error>> {
    let metadata = r#"{
        "packages": [
            {"name":"zeta","targets":[{"kind":["bin"]}]},
            {"name":"beta","targets":[{"kind":["cdylib","lib"]}]},
            {"name":"alpha","targets":[{"kind":["bin"]},{"kind":["lib"]}]},
            {"name":"gamma","targets":[{"kind":["proc-macro"]}]}
        ]
    }"#;
    let packages =
        verify_local_metadata::package_names(metadata, false).map_err(std::io::Error::other)?;
    let libraries =
        verify_local_metadata::package_names(metadata, true).map_err(std::io::Error::other)?;
    assert_eq!(packages, ["alpha", "beta", "gamma", "zeta"]);
    assert_eq!(libraries, ["alpha", "beta"]);
    assert!(verify_local_metadata::package_names("{}", false).is_err());
    Ok(())
}

#[test]
fn verify_local_toolchain_pins_match_native_mise_values() -> Result<(), Box<dyn Error>> {
    if Command::new("mise").arg("--version").output().is_err() {
        return Ok(());
    }
    for (mise_key, policy_key) in [
        ("tools.rust", "tools.rust"),
        ("tools.mr-boxington", "tools.mr-boxington"),
        (
            "tools.aqua:nextest-rs/nextest/cargo-nextest",
            "tools.nextest",
        ),
    ] {
        let mise_pin = native_mise_value("mise.toml", mise_key)?;
        let policy_pin = native_mise_value(".velnor/version-policy.toml", policy_key)?;
        assert_eq!(mise_pin, policy_pin, "pin drift for {mise_key}");
    }
    assert!(!native_mise_value(".velnor/version-policy.toml", "tools.mise")?.is_empty());
    Ok(())
}

/// Read a TOML value through the same native Mise command as the script.
fn native_mise_value(file: &str, key: &str) -> Result<String, Box<dyn Error>> {
    let output = Command::new("mise")
        .args(["config", "get", "--file", file, key])
        .current_dir(workspace_root())
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "mise config get failed for {file} {key}: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let value = String::from_utf8(output.stdout)?.trim().to_owned();
    if value.is_empty() {
        return Err(std::io::Error::other(format!(
            "mise config get returned an empty value for {file} {key}"
        ))
        .into());
    }
    Ok(value)
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
