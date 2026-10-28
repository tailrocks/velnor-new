// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};
use tempfile::TempDir;
use velnor_actions_contract::{Plan, ReleaseTarget, plan_json_bytes};
use velnor_actions_mise::current_mise_binary;
use velnor_actions_orchestrator::plan_internal;

const APPROVALS: &str = include_str!("../../../.velnor/controller-approvals.json");
const GENERATOR_LOCK: &str = include_str!("../../../.velnor/generator.lock");
const CHECK_ID: &str = "runtime-fixture";
const JOB_ID: &str = "check-runtime-fixture";
const RUN_KEY: &str = "r260001-a1";
const UNQUALIFIED_MISE: &[u8] = b"unqualified-runtime\n";

#[test]
fn exported_execute_check_binds_current_runtime_and_rejects_stale_plan()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let head = fixture.head()?;
    let config = fs::read(fixture.root().join(".velnor/config.toml"))?;
    let task = fs::read(fixture.root().join("mise.toml"))?;
    let original_plan = fixture.public_plan(&head)?;
    let original_bytes = plan_json_bytes(&original_plan)?;
    assert_cli_error(
        fixture.execute(&original_plan, &original_bytes)?,
        "mise_binary_unqualified_sha256",
    )?;

    fs::write(
        fixture.root().join(".velnor/controller-approvals.json"),
        compiled_runtime_approvals()?,
    )?;
    assert_eq!(
        fixture.head()?,
        head,
        "approval rotation must keep HEAD fixed"
    );
    assert_eq!(
        fs::read(fixture.root().join(".velnor/config.toml"))?,
        config
    );
    assert_eq!(fs::read(fixture.root().join("mise.toml"))?, task);
    let rotated_plan = fixture.public_plan(&head)?;
    let rotated_bytes = plan_json_bytes(&rotated_plan)?;
    assert_cli_error(
        fixture.execute(&rotated_plan, &rotated_bytes)?,
        "mise_binary_unqualified_sha256",
    )?;
    assert_cli_error(
        fixture.execute(&original_plan, &original_bytes)?,
        "check_plan_identity",
    )?;

    let (_, _, target) = host_platform()?;
    for mutation in [RuntimeMutation::Sha256, RuntimeMutation::Bytes] {
        eprintln!("checking same-version {mutation:?} runtime rotation");
        fs::write(
            fixture.root().join(".velnor/controller-approvals.json"),
            same_version_runtime_mutation(target, mutation)?,
        )?;
        let rotated_plan = fixture.public_plan(&head)?;
        let rotated_bytes = plan_json_bytes(&rotated_plan)?;
        assert_cli_error(
            fixture.execute(&rotated_plan, &rotated_bytes)?,
            "mise_binary_unqualified_sha256",
        )?;
        assert_cli_error(
            fixture.execute(&original_plan, &original_bytes)?,
            "check_plan_identity",
        )?;
    }
    assert_eq!(fixture.head()?, head);
    assert_eq!(
        fs::read(fixture.root().join(".velnor/config.toml"))?,
        config
    );
    assert_eq!(fs::read(fixture.root().join("mise.toml"))?, task);
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum RuntimeMutation {
    Sha256,
    Bytes,
}

struct Fixture {
    repository: TempDir,
    environment: TempDir,
    path: OsString,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let (runner_label, check_platform, target) = host_platform()?;
        let repository = tempfile::tempdir()?;
        let environment = tempfile::tempdir()?;
        let root = repository.path();
        create_checkout(root)?;
        fs::create_dir_all(root.join(".velnor"))?;
        fs::write(
            root.join(".velnor/config.toml"),
            config(runner_label, check_platform),
        )?;
        fs::write(root.join(".velnor/controller-approvals.json"), APPROVALS)?;
        fs::write(root.join(".velnor/generator.lock"), GENERATOR_LOCK)?;
        fs::write(
            root.join("mise.toml"),
            "[tasks.fixture-check]\nrun = \"true\"\n",
        )?;
        let bin = environment.path().join("bin");
        fs::create_dir_all(&bin)?;
        fs::write(bin.join("mise"), UNQUALIFIED_MISE)?;
        assert_unqualified_runtime_is_below_approval_bounds(target)?;
        let path = prefixed_path(&bin)?;
        fs::create_dir_all(environment.path().join("home"))?;
        Ok(Self {
            repository,
            environment,
            path,
        })
    }

    fn root(&self) -> &Path {
        self.repository.path()
    }

    fn head(&self) -> Result<String, Box<dyn Error>> {
        git_output(self.root(), &["rev-parse", "HEAD"])
    }

    fn public_plan(&self, head: &str) -> Result<Plan, Box<dyn Error>> {
        let request = json!({
            "schema": 1,
            "op": "plan-v1",
            "run_key": RUN_KEY,
            "base": null,
            "head": head,
            "event": "local",
            "root": self.root().to_string_lossy(),
        });
        let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
        Ok(serde_json::from_value(response["plan"].clone())?)
    }

    fn execute(&self, plan: &Plan, bytes: &[u8]) -> Result<Output, Box<dyn Error>> {
        let runner_temp = tempfile::tempdir()?;
        stage_plan(runner_temp.path(), bytes)?;
        let task_id = plan
            .obligations
            .iter()
            .find(|obligation| obligation.job_id == JOB_ID)
            .map(|obligation| obligation.task_id.as_str())
            .ok_or_else(|| io::Error::other("public plan check obligation missing"))?;
        Ok(Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", self.environment.path().join("home"))
            .env("TMPDIR", self.environment.path())
            .env("GITHUB_WORKSPACE", self.root())
            .env("RUNNER_TEMP", runner_temp.path())
            .env("GITHUB_RUN_ID", "260001")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("VELNOR_INTERNAL_OP", "execute-check-v1")
            .env("VELNOR_CHECK_ID", CHECK_ID)
            .env("VELNOR_TASK_ID", task_id)
            .env("VELNOR_CHECK_JOB_ID", JOB_ID)
            .env("VELNOR_CHECK_LANE_VARIANT", "single")
            .output()?)
    }
}

fn create_checkout(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"runtime-binding-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n")?;
    git(root, &["init", "-b", "main"])?;
    git(
        root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
    )?;
    git(root, &["config", "--local", "commit.gpgsign", "false"])?;
    git(root, &["config", "user.name", "Fixture"])?;
    git(root, &["config", "user.email", "fixture@example.invalid"])?;
    git(root, &["add", "Cargo.toml", "src/lib.rs"])?;
    git(root, &["commit", "-m", "runtime binding fixture"])
}

fn config(runner_label: &str, check_platform: &str) -> String {
    format!(
        r#"schema = 2
[workflow]
policy = "velnor-repository-v1"
default_branch = "main"
generator_validation = "bootstrap"
[discovery]
exclude = ["Cargo.toml"]
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
name = "fixture-scale-set"
labels = ["velnor", "fixture-scale-set"]
platform = "linux/amd64"
[[checks]]
id = "{CHECK_ID}"
task = "fixture-check"
inputs = ["Cargo.toml"]
tools = []
[checks.runner]
label = "{runner_label}"
platform = "{check_platform}"
executor = "hosted"
"#
    )
}

fn host_platform() -> Result<(&'static str, &'static str, ReleaseTarget), io::Error> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(("ubuntu-26.04", "linux_x64", ReleaseTarget::LinuxX86_64)),
        ("macos", "aarch64") => Ok(("macos-15", "macos_arm64", ReleaseTarget::MacosArm64)),
        ("macos", "x86_64") => Ok(("macos-15-intel", "macos_x64", ReleaseTarget::MacosX86_64)),
        (os, arch) => Err(io::Error::other(format!("unsupported host {os}/{arch}"))),
    }
}

fn compiled_runtime_approvals() -> Result<String, Box<dyn Error>> {
    let mut value: Value = serde_json::from_str(APPROVALS)?;
    let entries = value
        .pointer_mut("/active/mise_runtime")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("active runtime approvals missing"))?;
    for target in ReleaseTarget::ALL {
        let pin = current_mise_binary(target);
        let entry = entries
            .iter_mut()
            .find(|entry| entry["target"].as_str() == Some(target.triple()))
            .ok_or_else(|| io::Error::other("runtime target approval missing"))?;
        entry["version"] = Value::String(pin.version.to_owned());
        entry["sha256"] = Value::String(pin.sha256.to_owned());
        entry["bytes"] = Value::from(pin.bytes);
    }
    Ok(serde_json::to_string(&value)?)
}

fn same_version_runtime_mutation(
    target: ReleaseTarget,
    mutation: RuntimeMutation,
) -> Result<String, Box<dyn Error>> {
    let mut approvals: Value = serde_json::from_str(APPROVALS)?;
    let entries = approvals
        .pointer_mut("/active/mise_runtime")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("active runtime approvals missing"))?;
    let entry = entries
        .iter_mut()
        .find(|entry| entry["target"].as_str() == Some(target.triple()))
        .ok_or_else(|| io::Error::other("host runtime approval missing"))?;
    match mutation {
        RuntimeMutation::Sha256 => {
            let current = entry["sha256"]
                .as_str()
                .ok_or_else(|| io::Error::other("runtime SHA-256 missing"))?;
            let changed_first = if current.starts_with('0') { '1' } else { '0' };
            entry["sha256"] = Value::String(format!("{changed_first}{}", &current[1..]));
        }
        RuntimeMutation::Bytes => {
            let current = entry["bytes"]
                .as_u64()
                .ok_or_else(|| io::Error::other("runtime byte count missing"))?;
            entry["bytes"] = Value::from(
                current
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("runtime byte count overflow"))?,
            );
        }
    }
    Ok(serde_json::to_string(&approvals)?)
}

fn assert_unqualified_runtime_is_below_approval_bounds(
    target: ReleaseTarget,
) -> Result<(), Box<dyn Error>> {
    let approved = approved_bytes(APPROVALS, target)?;
    let rotated = compiled_runtime_approvals()?;
    let compiled = approved_bytes(&rotated, target)?;
    assert!((UNQUALIFIED_MISE.len() as u64) < approved);
    assert!((UNQUALIFIED_MISE.len() as u64) < compiled);
    Ok(())
}

fn approved_bytes(approvals: &str, target: ReleaseTarget) -> Result<u64, Box<dyn Error>> {
    let value: Value = serde_json::from_str(approvals)?;
    value
        .pointer("/active/mise_runtime")
        .and_then(Value::as_array)
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["target"].as_str() == Some(target.triple()))
        })
        .and_then(|entry| entry["bytes"].as_u64())
        .ok_or_else(|| io::Error::other("host runtime approval byte bound missing").into())
}

fn prefixed_path(bin: &Path) -> Result<OsString, Box<dyn Error>> {
    let original = std::env::var_os("PATH").unwrap_or_default();
    let entries = std::iter::once(PathBuf::from(bin)).chain(std::env::split_paths(&original));
    Ok(std::env::join_paths(entries)?)
}

fn stage_plan(runner_temp: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let dir = runner_temp.join("velnor").join(RUN_KEY);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("plan.json"), bytes)?;
    Ok(())
}

fn assert_cli_error(output: Output, marker: &str) -> Result<(), Box<dyn Error>> {
    assert_eq!(
        output.status.code(),
        Some(1),
        "CLI must reject the staged plan"
    );
    let stderr = String::from_utf8(output.stderr)?;
    let expected = format!("velnor-actions: internal request failed: internal: {marker}");
    assert_eq!(stderr.trim(), expected, "unexpected public CLI error");
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
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

fn git_output(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into())
    }
}
