// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, io, path::Path, process::Command};

use tempfile::TempDir;
use velnor_actions_orchestrator::prepare;

const CONFIG: &str = r#"schema = 2
[workflow]
policy = "velnor-repository-v1"
default_branch = "main"
generator_validation = "bootstrap"
[stacks.rust]
test_runner = "cargo_nextest"
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
[[native_checks]]
id = "fixture-native-tree"
operation = "tap_tree"
[native_checks.runner]
label = "ubuntu-26.04"
platform = "linux_x64"
executor = "hosted"
[[checks]]
id = "fixture-mise-linux"
task = "fixture-check"
inputs = ["Cargo.toml"]
tools = []
[checks.runner]
label = "ubuntu-26.04"
platform = "linux_x64"
executor = "hosted"
[[checks]]
id = "fixture-mise-arm"
task = "fixture-check"
inputs = ["Cargo.toml"]
tools = []
[checks.runner]
label = "macos-15"
platform = "macos_arm64"
executor = "hosted"
[[checks]]
id = "fixture-mise-intel"
task = "fixture-check"
inputs = ["Cargo.toml"]
tools = []
[checks.runner]
label = "macos-15-intel"
platform = "macos_x64"
executor = "hosted"
"#;

const BASE_APPROVALS: &str = include_str!("../../../.velnor/controller-approvals.json");
const BASE_LOCK: &str = include_str!("../../../.velnor/generator.lock");

fn fixture(approvals: Option<&str>, lock: &str) -> Result<TempDir, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"approval-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n")?;
    git(&["init", "-b", "main"], root)?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
        root,
    )?;
    git(&["config", "--local", "commit.gpgsign", "false"], root)?;
    git(&["config", "user.name", "Fixture"], root)?;
    git(&["config", "user.email", "fixture@example.invalid"], root)?;
    git(&["add", "Cargo.toml", "src/lib.rs"], root)?;
    git(&["commit", "-m", "fixture base"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), CONFIG)?;
    fs::write(
        root.join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    fs::write(root.join(".velnor/generator.lock"), lock)?;
    if let Some(approvals) = approvals {
        fs::write(root.join(".velnor/controller-approvals.json"), approvals)?;
    }
    Ok(temp)
}

fn git(args: &[&str], root: &Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into())
    }
}

fn prepare_error(root: &Path) -> Result<String, Box<dyn Error>> {
    match prepare(root) {
        Ok(_) => Err(io::Error::other("prepare unexpectedly succeeded").into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn missing_approvals_file_fails_closed() -> Result<(), Box<dyn Error>> {
    let temp = fixture(None, BASE_LOCK)?;
    let error = prepare_error(temp.path())?;
    assert!(
        error.contains("controller_approvals:missing_approval"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn malformed_approvals_fail_closed() -> Result<(), Box<dyn Error>> {
    let temp = fixture(Some("{not json"), BASE_LOCK)?;
    let error = prepare_error(temp.path())?;
    assert!(
        error.contains("controller_approvals:malformed_json"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn foreign_repository_approval_is_rejected() -> Result<(), Box<dyn Error>> {
    let tampered = BASE_APPROVALS.replace("tailrocks/velnor-new", "attacker/velnor-new");
    let temp = fixture(Some(&tampered), BASE_LOCK)?;
    let error = prepare_error(temp.path())?;
    assert!(
        error.contains("controller_approvals:unexpected_repository"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn tampered_active_commit_is_rejected() -> Result<(), Box<dyn Error>> {
    let tampered = BASE_APPROVALS.replacen(
        "693f3aae14406fb7851ac70f0aee647a869b5063",
        "693f3aae14406fb7851ac70f0aee647a869b5064",
        1,
    );
    let temp = fixture(Some(&tampered), BASE_LOCK)?;
    let error = prepare_error(temp.path())?;
    assert!(
        error.contains("controller_approvals:manifest_digest_mismatch"),
        "unexpected error: {error}"
    );
    Ok(())
}

const STAGED_ENTRY: &str = r#"{
    "repository": "tailrocks/velnor-new",
    "version": "0.1.60",
    "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "manifest_digest": "sha256:7c3653d3ad7448241bd6b8078ab80ba4f3a8274f602e37ede209572c0a01a0a4",
    "binaries": [
      {
        "target": "x86_64-unknown-linux-gnu",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.60/velnor-actions-0.1.60-x86_64-unknown-linux-gnu",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      },
      {
        "target": "aarch64-apple-darwin",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.60/velnor-actions-0.1.60-aarch64-apple-darwin",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      },
      {
        "target": "x86_64-apple-darwin",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.60/velnor-actions-0.1.60-x86_64-apple-darwin",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      }
    ],
    "gh": "2.103.2",
    "mise": {
      "version": "2026.10.21",
      "artifact": "https://github.com/jdx/mise/releases/download/v2026.10.21/mise-v2026.10.21-linux-x64",
      "sha256": "ccfe12d7a62c6fb9def2a20db1f210f13a51de881f21fd8c85db4b6312405d07"
    },
    "state": "staged"
  }"#;

#[test]
fn staged_entry_stays_inert() -> Result<(), Box<dyn Error>> {
    let with_staged =
        BASE_APPROVALS.replace("\"staged\": null", &format!("\"staged\": {STAGED_ENTRY}"));
    let temp = fixture(Some(&with_staged), BASE_LOCK)?;
    prepare(temp.path())
        .map_err(|error| io::Error::other(format!("staged entry must stay inert: {error}")))?;
    Ok(())
}

const ROTATED_SHA: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn rotated_approvals() -> String {
    BASE_APPROVALS
        .replacen("\"version\": \"0.1.55\"", "\"version\": \"0.1.60\"", 1)
        .replacen(
            "693f3aae14406fb7851ac70f0aee647a869b5063",
            "dddddddddddddddddddddddddddddddddddddddd",
            1,
        )
        .replace(
            "sha256:90ee052ab599c9c064afce1d26b65e6752c4b7cd8feecd3c57249ea0241afbd3",
            "sha256:facc89e97684d9979c05c09401fb2f627cd4bc782fdfa7efead6429874050c39",
        )
        .replace("v0.1.55", "v0.1.60")
        .replace("0.1.55-", "0.1.60-")
        .replace(
            "c8f1440e4f767ecc903e6611c56c2db4005011d33ee6f0fd566f4b7e5345eba4",
            ROTATED_SHA,
        )
        .replace(
            "eb8690b6a05b18bf025a0d7876c8497cedc04d54df04fdb5a3c20d203478e756",
            ROTATED_SHA,
        )
        .replace(
            "0938a574c00b05a71735285bf0d2840235ca4f9559359909564dd53b27d77883",
            ROTATED_SHA,
        )
}

fn lock_for_version(version: &str, commit: &str) -> String {
    let tag = format!("v{version}");
    let binaries = [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ]
    .map(|target| {
        format!(
            "\n[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://github.com/tailrocks/velnor-new/releases/download/{tag}/velnor-actions-{version}-{target}\"\nsha256 = \"ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"\n"
        )
    })
    .join("");
    format!(
        "schema = 1\n\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\ncommit = \"{commit}\"\n{binaries}\n[mise-bootstrap]\nversion = \"2026.10.21\"\nartifact = \"https://github.com/jdx/mise/releases/download/v2026.10.21/mise-v2026.10.21-linux-x64\"\nsha256 = \"ccfe12d7a62c6fb9def2a20db1f210f13a51de881f21fd8c85db4b6312405d07\"\n"
    )
}

#[test]
fn non_controller_workflow_is_rejected() -> Result<(), Box<dyn Error>> {
    let tampered = BASE_APPROVALS.replace(
        ".github/workflows/product-release-generator.yml",
        ".github/workflows/ci.yml",
    );
    let temp = fixture(Some(&tampered), BASE_LOCK)?;
    let error = prepare_error(temp.path())?;
    assert!(
        error.contains("controller_approvals:unexpected_workflow"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn stale_manifest_digest_is_rejected() -> Result<(), Box<dyn Error>> {
    let tampered = BASE_APPROVALS
        .replacen("\"version\": \"0.1.55\"", "\"version\": \"0.1.60\"", 1)
        .replace("v0.1.55", "v0.1.60")
        .replace("0.1.55-", "0.1.60-");
    let temp = fixture(Some(&tampered), BASE_LOCK)?;
    let error = prepare_error(temp.path())?;
    assert!(
        error.contains("controller_approvals:manifest_digest_mismatch"),
        "unexpected error: {error}"
    );
    Ok(())
}

const ROTATED_MISE_VERSION: &str = "2026.10.22";
const ROTATED_MISE_SHA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn approvals_with_mise(version: &str, sha: &str) -> String {
    BASE_APPROVALS
        .replace(
            "\"version\": \"2026.10.21\"",
            &format!("\"version\": \"{version}\""),
        )
        .replace(
            "ccfe12d7a62c6fb9def2a20db1f210f13a51de881f21fd8c85db4b6312405d07",
            sha,
        )
        .replace("v2026.10.21", &format!("v{version}"))
}

fn lock_with_mise_bootstrap(version: &str, sha: &str) -> String {
    BASE_LOCK
        .replace(
            "version = \"2026.10.21\"",
            &format!("version = \"{version}\""),
        )
        .replace(
            "ccfe12d7a62c6fb9def2a20db1f210f13a51de881f21fd8c85db4b6312405d07",
            sha,
        )
        .replace("v2026.10.21", &format!("v{version}"))
}

#[test]
fn rotated_mise_identity_governs_lock() -> Result<(), Box<dyn Error>> {
    let approvals = approvals_with_mise(ROTATED_MISE_VERSION, ROTATED_MISE_SHA);
    let lock = lock_with_mise_bootstrap(ROTATED_MISE_VERSION, ROTATED_MISE_SHA);
    let temp = fixture(Some(&approvals), &lock)?;
    prepare(temp.path()).map_err(|error| {
        io::Error::other(format!("matching rotated mise must be accepted: {error}"))
    })?;
    let stale_lock = lock_with_mise_bootstrap(
        "2026.10.21",
        "ccfe12d7a62c6fb9def2a20db1f210f13a51de881f21fd8c85db4b6312405d07",
    );
    let temp_old = fixture(Some(&approvals), &stale_lock)?;
    let error = prepare_error(temp_old.path())?;
    assert!(
        error.contains("lock_mismatch:mise_bootstrap"),
        "rotated mise must reject the old bootstrap: {error}"
    );
    Ok(())
}

#[test]
fn rotated_authority_accepts_new_and_rejects_old() -> Result<(), Box<dyn Error>> {
    let commit = "dddddddddddddddddddddddddddddddddddddddd";
    let approvals = rotated_approvals();
    let new_lock = lock_for_version("0.1.60", commit);
    let temp = fixture(Some(&approvals), &new_lock)?;
    prepare(temp.path()).map_err(|error| {
        io::Error::other(format!(
            "rotated authority must accept its own lock: {error}"
        ))
    })?;
    let old_lock = lock_for_version("0.1.55", "693f3aae14406fb7851ac70f0aee647a869b5063");
    let temp_old = fixture(Some(&approvals), &old_lock)?;
    let error = prepare_error(temp_old.path())?;
    assert!(
        error.contains("lock_mismatch"),
        "rotated authority must reject the old lock: {error}"
    );
    Ok(())
}
