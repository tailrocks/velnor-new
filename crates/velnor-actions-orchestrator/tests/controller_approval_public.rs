// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, io, path::Path, process::Command};

use tempfile::TempDir;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

#[path = "../../test_support/root_cargo_lock.rs"]
mod root_cargo_lock;

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
    root_cargo_lock::write_root_cargo_lock(root, &["approval-fixture"])?;
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
    git(&["add", "Cargo.toml", "Cargo.lock", "src/lib.rs"], root)?;
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

fn job_block<'a>(workflow: &'a str, job: &str) -> Result<&'a str, Box<dyn Error>> {
    let header = format!("\n  {job}:\n");
    let start = workflow
        .find(&header)
        .ok_or_else(|| io::Error::other(format!("job {job} is not rendered")))?
        + 1;
    let body = &workflow[start..];
    let length = body
        .split_inclusive('\n')
        .enumerate()
        .skip(1)
        .find(|(_, line)| !line.trim().is_empty() && !line.starts_with("    "))
        .map_or(body.len(), |(index, _)| {
            body.split_inclusive('\n').take(index).map(str::len).sum()
        });
    Ok(&body[..length])
}

#[test]
fn required_verdict_controller_is_the_active_approved_release() -> Result<(), Box<dyn Error>> {
    let temp = fixture(Some(BASE_APPROVALS), BASE_LOCK)?;
    let tree = render_staged_tree(&prepare(temp.path())?)?;
    let workflow = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| io::Error::other("ci workflow is not rendered"))?;
    let required = job_block(workflow, "required")?;
    let approvals: serde_json::Value = serde_json::from_str(BASE_APPROVALS)?;
    let active = &approvals["active"];
    let version = active["version"]
        .as_str()
        .ok_or_else(|| io::Error::other("active version is missing"))?;
    let linux = active["binaries"]
        .as_array()
        .and_then(|binaries| {
            binaries
                .iter()
                .find(|binary| binary["target"] == "x86_64-unknown-linux-gnu")
        })
        .ok_or_else(|| io::Error::other("active linux binary is missing"))?;
    let artifact = linux["artifact"]
        .as_str()
        .ok_or_else(|| io::Error::other("active artifact is missing"))?;
    let sha256 = linux["sha256"]
        .as_str()
        .ok_or_else(|| io::Error::other("active sha256 is missing"))?;
    assert!(
        required.contains(&format!("VELNOR_ASSET_URL: {artifact}"))
            && required.contains(&format!("VELNOR_ASSET_SHA256: {sha256}")),
        "Required verdict job does not acquire the active approved release {version}:\n{required}"
    );
    Ok(())
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
        "c622bfb187556afb8d3ff3ca917a719e4bb7f32a",
        "d622bfb187556afb8d3ff3ca917a719e4bb7f32a",
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
    "version": "0.1.75",
    "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "manifest_digest": "sha256:715c8951a8a6bcbb7f17040e67b17350dab014a48ed113d26a96fdd0df8d02c8",
    "binaries": [
      {
        "target": "x86_64-unknown-linux-gnu",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.75/velnor-actions-0.1.75-x86_64-unknown-linux-gnu",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      },
      {
        "target": "aarch64-apple-darwin",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.75/velnor-actions-0.1.75-aarch64-apple-darwin",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      },
      {
        "target": "x86_64-apple-darwin",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.75/velnor-actions-0.1.75-x86_64-apple-darwin",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      }
    ],
    "gh": "2.103.2",
    "mise_bootstrap": {
      "version": "2026.11.1",
      "artifact": "https://github.com/jdx/mise/releases/download/v2026.11.1/mise-v2026.11.1-linux-x64",
      "sha256": "7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9"
    },
    "mise_runtime": [
      {"target": "x86_64-unknown-linux-gnu", "version": "2026.10.20", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "bytes": 173449633},
      {"target": "aarch64-apple-darwin", "version": "2026.10.20", "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "bytes": 138238817},
      {"target": "x86_64-apple-darwin", "version": "2026.10.20", "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc", "bytes": 169788225}
    ],
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
        .replacen("\"version\": \"0.1.74\"", "\"version\": \"0.1.75\"", 1)
        .replacen(
            "c622bfb187556afb8d3ff3ca917a719e4bb7f32a",
            "dddddddddddddddddddddddddddddddddddddddd",
            1,
        )
        .replace(
            "sha256:fb43e14dc28afd2535e36bb71c129dbb7199b5a15dd26a853b010cb549b03e07",
            "sha256:caf5b082bbea2f15abd2d3afa09909d26de8ecd78427f1027dab300f42c41492",
        )
        .replace("v0.1.74", "v0.1.75")
        .replace("0.1.74-", "0.1.75-")
        .replace(
            "db7480b83238f85a2e4472d26ba57ca4ee4d6822353cbdaad690b1a2b963b26f",
            ROTATED_SHA,
        )
        .replace(
            "0a97e53bdd5faada42c0e8bedce2c54f710ada669cdd58cc918aaac80df68772",
            ROTATED_SHA,
        )
        .replace(
            "41d55fe769f40df114ace812a19566d5c188d99143f0a631299dc10ebed4c09c",
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
        "schema = 1\n\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\ncommit = \"{commit}\"\n{binaries}\n[mise-bootstrap]\nversion = \"2026.11.1\"\nartifact = \"https://github.com/jdx/mise/releases/download/v2026.11.1/mise-v2026.11.1-linux-x64\"\nsha256 = \"7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9\"\n"
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
        .replacen("\"version\": \"0.1.74\"", "\"version\": \"0.1.75\"", 1)
        .replace("v0.1.74", "v0.1.75")
        .replace("0.1.74-", "0.1.75-");
    let temp = fixture(Some(&tampered), BASE_LOCK)?;
    let error = prepare_error(temp.path())?;
    assert!(
        error.contains("controller_approvals:manifest_digest_mismatch"),
        "unexpected error: {error}"
    );
    Ok(())
}

const ROTATED_MISE_VERSION: &str = "2026.11.2";
const ROTATED_MISE_SHA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn approvals_with_mise(version: &str, sha: &str) -> String {
    BASE_APPROVALS
        .replace(
            "\"version\": \"2026.11.1\"",
            &format!("\"version\": \"{version}\""),
        )
        .replace(
            "7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9",
            sha,
        )
        .replace("v2026.11.1", &format!("v{version}"))
}

fn lock_with_mise_bootstrap(version: &str, sha: &str) -> String {
    BASE_LOCK
        .replace(
            "version = \"2026.11.1\"",
            &format!("version = \"{version}\""),
        )
        .replace(
            "7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9",
            sha,
        )
        .replace("v2026.11.1", &format!("v{version}"))
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
        "2026.11.1",
        "7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9",
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
    let new_lock = lock_for_version("0.1.75", commit);
    let temp = fixture(Some(&approvals), &new_lock)?;
    prepare(temp.path()).map_err(|error| {
        io::Error::other(format!(
            "rotated authority must accept its own lock: {error}"
        ))
    })?;
    let temp_old = fixture(Some(&approvals), BASE_LOCK)?;
    let error = prepare_error(temp_old.path())?;
    assert!(
        error.contains("lock_mismatch"),
        "rotated authority must reject the old lock: {error}"
    );
    Ok(())
}

#[test]
fn symlinked_approvals_are_rejected() -> Result<(), Box<dyn Error>> {
    let temp = fixture(Some(BASE_APPROVALS), BASE_LOCK)?;
    let approvals = temp.path().join(".velnor/controller-approvals.json");
    let outside = temp.path().join("outside-approvals.json");
    fs::write(&outside, BASE_APPROVALS)?;
    fs::remove_file(&approvals)?;
    std::os::unix::fs::symlink(&outside, &approvals)?;
    let error = prepare_error(temp.path())?;
    assert!(
        error.contains("symlink"),
        "symlinked approvals must fail closed: {error}"
    );
    Ok(())
}

#[path = "controller_approval_public/runtime.rs"]
mod runtime_public;
