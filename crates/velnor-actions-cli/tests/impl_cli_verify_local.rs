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
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::process::Command;
#[cfg(unix)]
use std::time::{SystemTime, UNIX_EPOCH};

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
        [
            "toolchain",
            "generated-selector",
            "generated-tree",
            "fixtures",
            "integration",
        ],
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
    assert!(
        !script.contains("/tmp/verify-local-"),
        "logs are allocated per invocation"
    );
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

#[cfg(unix)]
fn verify_local_log_parent() -> Result<PathBuf, Box<dyn Error>> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let parent = std::env::temp_dir().join(format!(
        "velnor-verify-local-logs-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&parent)?;
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o2700))?;
    let bin = parent.join("bin");
    fs::create_dir(&bin)?;
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o700))?;
    let mise = bin.join("mise");
    fs::write(
        &mise,
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '2026.10.6\\n' ;;\n  install) printf '%s\\n' \"$VERIFY_LOCAL_LOG_TEST_MARKER\" >&2; exit 1 ;;\n  *) exit 2 ;;\nesac\n",
    )?;
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o700))?;
    Ok(parent)
}

#[cfg(unix)]
fn run_verify_local_failure(
    parent: &std::path::Path,
    marker: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    let bin = parent.join("bin");
    let mut path = std::ffi::OsString::from(bin);
    path.push(":");
    path.push(std::env::var_os("PATH").ok_or("PATH is missing")?);
    Ok(Command::new("bash")
        .arg(workspace_root().join("scripts/verify-local.sh"))
        .env("TMPDIR", parent)
        .env("PATH", path)
        .env("VERIFY_LOCAL_LOG_TEST_MARKER", marker)
        .output()?)
}

#[cfg(unix)]
fn verify_local_log_dir(output: &[u8]) -> Result<PathBuf, Box<dyn Error>> {
    let output = String::from_utf8_lossy(output);
    let path = output
        .lines()
        .find_map(|line| line.strip_prefix("verify-local: log directory: "))
        .ok_or("verify-local did not print its log directory")?;
    Ok(PathBuf::from(path))
}

#[cfg(unix)]
fn current_effective_uid() -> Result<u32, Box<dyn Error>> {
    let output = Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err("id -u failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().parse()?)
}

#[cfg(unix)]
#[test]
fn verify_local_failure_logs_are_private_and_isolated() -> Result<(), Box<dyn Error>> {
    let parent = verify_local_log_parent()?;
    let first = run_verify_local_failure(&parent, "failure-A")?;
    assert_eq!(first.status.code(), Some(1), "first run failure exit");
    let first_dir = verify_local_log_dir(&first.stdout)?;
    let first_log = first_dir.join("toolchain.log");
    let first_output = String::from_utf8_lossy(&first.stdout);
    assert!(first_output.contains(&first_log.display().to_string()));
    assert!(fs::read_to_string(&first_log)?.contains("failure-A"));
    let first_meta = fs::metadata(&first_dir)?;
    assert_eq!(first_meta.permissions().mode() & 0o777, 0o700);
    assert_eq!(first_meta.uid(), current_effective_uid()?);

    let second = run_verify_local_failure(&parent, "failure-B")?;
    assert_eq!(second.status.code(), Some(1), "second run failure exit");
    let second_dir = verify_local_log_dir(&second.stdout)?;
    let second_log = second_dir.join("toolchain.log");
    assert_ne!(first_dir, second_dir, "invocations use unique directories");
    assert!(fs::read_to_string(&second_log)?.contains("failure-B"));
    assert_eq!(fs::read_to_string(&first_log)?.trim(), "failure-A");
    assert_eq!(
        fs::metadata(&second_dir)?.permissions().mode() & 0o777,
        0o700
    );
    let parent_mode = fs::metadata(&parent)?.permissions().mode() & 0o7777;
    assert_eq!(parent_mode, 0o2700, "the caller's SGID parent is preserved");

    fs::remove_dir_all(parent)?;
    Ok(())
}
