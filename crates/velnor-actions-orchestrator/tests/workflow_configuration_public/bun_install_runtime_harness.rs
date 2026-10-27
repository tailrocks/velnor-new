// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::super::super::workflow_configuration_fixture;
use velnor_actions_contract::StepKind;
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

const BUN_VERSION: &str = "1.4.4";
const LEGACY_INSTALL: &str =
    include_str!("../../../../fixtures/bun-install-lifecycle-runtime/legacy-install-step.sh");

pub(super) fn pinned_bun() -> Result<PathBuf, Box<dyn Error>> {
    let mise = find_host_executable("mise")?;
    let output = Command::new(mise)
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "where",
            "bun@1.4.4",
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "pinned Bun {BUN_VERSION} is unavailable: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let path = PathBuf::from(String::from_utf8(output.stdout)?.trim()).join("bin/bun");
    let version = Command::new(&path).arg("--version").output()?;
    if !version.status.success() || String::from_utf8_lossy(&version.stdout).trim() != BUN_VERSION {
        return Err(format!("expected real Bun {BUN_VERSION} at {}", path.display()).into());
    }
    Ok(path)
}

pub(super) fn make_repo(private: bool) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let extra = if private {
        "\n[private_sources]\nowner = \"fixture-owner\"\napp_id_variable = \"FIXTURE_APP_ID\"\napp_private_key_secret = \"FIXTURE_APP_PRIVATE_KEY\"\nnpm_scopes = [\"@fixture\"]\n"
    } else {
        ""
    };
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.bun]\nroots = [\"frontend\"]\nscripts = [\"probe\"]\n{extra}"
    );
    let root = workflow_configuration_fixture::make_repo(&config)?;
    copy_workspace_fixture(root.path())?;
    copy_fixture_file(
        &root.path().join("frontend/mise.toml"),
        &root.path().join("mise.toml"),
    )?;
    if private {
        fs::write(root.path().join(".velnor/deps.lock"), "schema = 1\n")?;
    }
    Ok(root)
}

fn copy_workspace_fixture(root: &Path) -> Result<(), Box<dyn Error>> {
    let fixture = fixture_root();
    let destination = root.join("frontend");
    fs::create_dir_all(&destination)?;
    for name in ["package.json", "bun.lock", "mise.toml"] {
        copy_fixture_file(&fixture.join(name), &destination.join(name))?;
    }
    for package in ["runtime-probe", "runtime-probe-two"] {
        copy_fixture_file(
            &fixture.join(package).join("package.json"),
            &destination.join(package).join("package.json"),
        )?;
    }
    for file in [
        "runtime-probe/runtime.cjs",
        "runtime-probe-two/bun.cjs",
        "script-shadow.cjs",
    ] {
        copy_fixture_file(&fixture.join(file), &destination.join(file))?;
    }
    Ok(())
}

pub(super) fn generated_install(
    root: &Path,
) -> Result<(GenerationPreparation, String), Box<dyn Error>> {
    let prepared = prepare(root)?;
    let rendered = render_staged_tree(&prepared)?;
    assert!(rendered.get(WORKFLOW_PATH).is_some());
    let install = prepared
        .workflow
        .ir
        .jobs
        .values()
        .flat_map(|job| &job.steps)
        .find(|step| step.name == "Install dependencies")
        .ok_or("generated install step missing")?;
    let script = shell_script(install)?;
    Ok((prepared, script))
}

pub(super) fn generated_private_lifecycle(
    root: &Path,
) -> Result<(GenerationPreparation, String), Box<dyn Error>> {
    let prepared = prepare(root)?;
    let rendered = render_staged_tree(&prepared)?;
    assert!(rendered.get(WORKFLOW_PATH).is_some());
    let install = prepared
        .workflow
        .ir
        .jobs
        .values()
        .flat_map(|job| &job.steps)
        .find(|step| step.name == "Install dependencies (lifecycle phase)")
        .ok_or("private lifecycle step missing")?;
    let script = shell_script(install)?;
    Ok((prepared, script))
}

fn shell_script(step: &velnor_actions_contract::Step) -> Result<String, Box<dyn Error>> {
    match &step.kind {
        StepKind::Shell { run, .. } if run.len() == 3 => Ok(run[2].clone()),
        _ => Err("generated Bun install is not a bash argv".into()),
    }
}

pub(super) fn frozen_legacy_install_arm(bun: &Path) -> Result<(), Box<dyn Error>> {
    let root = make_repo(false)?;
    let lock = fs::read(root.path().join("frontend/bun.lock"))?;
    let output = run_generated(root.path(), LEGACY_INSTALL, bun, "legacy")?;
    assert_eq!(
        output.status.code(),
        Some(77),
        "frozen legacy install changed: {output:?}"
    );
    assert!(fs::read_to_string(root.path().join("node-sentinel.log"))?.contains("runtime.cjs"));
    assert!(!root.path().join("runtime-receipt.json").exists());
    assert_eq!(fs::read(root.path().join("frontend/bun.lock"))?, lock);
    Ok(())
}

pub(super) fn frozen_lock_mismatch_arm(bun: &Path) -> Result<(), Box<dyn Error>> {
    let root = make_repo(false)?;
    let (_, script) = generated_install(root.path())?;
    let manifest_path = root.path().join("frontend/package.json");
    let mut package: serde_json::Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    package["dependencies"]["runtime-probe-three"] =
        serde_json::json!("file:./runtime-probe-three");
    fs::write(&manifest_path, serde_json::to_vec(&package)?)?;
    copy_fixture_file(
        &fixture_root().join("runtime-probe-three/package.json"),
        &root
            .path()
            .join("frontend/runtime-probe-three/package.json"),
    )?;
    let lock = fs::read(root.path().join("frontend/bun.lock"))?;
    let output = run_generated(root.path(), &script, bun, "stale-lock")?;
    assert!(
        !output.status.success(),
        "frozen install accepted manifest change: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("lockfile"),
        "frozen error missing: {output:?}"
    );
    assert!(!root.path().join("runtime-receipt.json").exists());
    assert_eq!(
        fs::read_to_string(root.path().join("node-sentinel.log"))?,
        ""
    );
    assert_eq!(fs::read(root.path().join("frontend/bun.lock"))?, lock);
    Ok(())
}

pub(super) fn run_generated(
    root: &Path,
    script: &str,
    bun: &Path,
    label: &str,
) -> Result<Output, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let bin = temp.path().join("bin");
    fs::create_dir(&bin)?;
    for name in ["bash", "sh", "find", "mkdir", "rm"] {
        symlink(find_host_executable(name)?, bin.join(name))?;
    }
    symlink(bun, bin.join("bun"))?;
    write_node_sentinel(&bin.join("node"), 77)?;
    write_mise_stub(&bin.join("mise"))?;
    let home = temp.path().join("home");
    let runner_temp = temp.path().join("runner-temp");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&runner_temp)?;
    for log in ["node-sentinel.log", "mise-stub.log", "shadow-log"] {
        fs::write(root.join(log), b"")?;
    }
    if label == "private" {
        let config = runner_temp.join("velnor/bun/lifecycle.toml");
        fs::create_dir_all(config.parent().ok_or("lifecycle config parent missing")?)?;
        fs::write(config, "[install]\n")?;
    }
    let mut command = Command::new(bin.join("bash"));
    command.args(["-c", script]).current_dir(root).env_clear();
    command
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("TMPDIR", temp.path());
    command
        .env("GITHUB_WORKSPACE", root)
        .env("RUNNER_TEMP", &runner_temp);
    command
        .env("VELNOR_TOOL_BUN", BUN_VERSION)
        .env("REAL_BUN", bun);
    command.env("MISE_STUB_LOG", root.join("mise-stub.log"));
    command.env("NODE_SENTINEL_LOG", root.join("node-sentinel.log"));
    command.env("LIFECYCLE_RECEIPT", root.join("runtime-receipt.json"));
    command.env("SHADOW_LOG", root.join("shadow-log"));
    command.env("MISE_EXEC_AUTO_INSTALL", "0");
    command.env("BUN_INSTALL_CACHE_DIR", temp.path().join("bun-cache"));
    let output = command.output()?;
    if fs::read(bin.join("node"))? != node_sentinel_bytes(77) {
        return Err("controlled node stub changed during installation".into());
    }
    Ok(output)
}

pub(super) fn assert_runtime_success(
    root: &Path,
    bun: &Path,
    output: &Output,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    if !output.status.success() {
        return Err(format!("{label} install failed: {output:?}").into());
    }
    assert_runtime_receipt(root, bun)?;
    if !fs::read_to_string(root.join("node-sentinel.log"))?.is_empty() {
        return Err(format!("{label} installation invoked the controlled node stub").into());
    }
    Ok(())
}

fn assert_runtime_receipt(root: &Path, bun: &Path) -> Result<(), Box<dyn Error>> {
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("runtime-receipt.json"))?)?;
    if receipt["bun"] != BUN_VERSION {
        return Err(format!("expected Bun {BUN_VERSION}: {receipt}").into());
    }
    if receipt["execPath"] != bun.to_string_lossy().as_ref() {
        return Err(format!("unexpected runtime executable: {receipt}").into());
    }
    if !receipt["argv"].as_array().is_some_and(|argv| {
        argv.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|arg| arg.ends_with("runtime.cjs"))
        })
    }) {
        return Err(format!("runtime receipt does not identify runtime.cjs: {receipt}").into());
    }
    Ok(())
}

fn write_mise_stub(path: &Path) -> Result<(), Box<dyn Error>> {
    let source = r#"#!/bin/sh
set -eu
[ "$#" -ge 7 ] || exit 91
[ "$1" = --no-config ] && [ "$2" = --no-env ] && [ "$3" = --no-hooks ] && [ "$4" = exec ] && [ "$5" = bun@1.4.4 ] && [ "$6" = -- ] && [ "$7" = bun ] || exit 92
shift 7
if [ "$1" = --bun ] && [ "$2" = run ] && [ "$3" = - ]; then
  mode=stdin
else
  mode=other
fi
printf '%s\n' "$mode" >> "$MISE_STUB_LOG"
exec "$REAL_BUN" "$@"
"#;
    fs::write(path, source)?;
    set_executable(path)
}

fn write_node_sentinel(path: &Path, exit: u8) -> Result<(), Box<dyn Error>> {
    fs::write(path, node_sentinel_bytes(exit))?;
    set_executable(path)
}

fn node_sentinel_bytes(exit: u8) -> Vec<u8> {
    format!(
        r#"#!/bin/sh
if [ "${{1-}}" = --version ] || [ "${{1-}}" = -v ]; then
  printf '%s\n' "version-probe|$*" >> "$NODE_SENTINEL_LOG"
  printf '%s\n' 'v26.12.0'
  exit 0
fi
printf '%s\n' "lifecycle|$(pwd)|$*" >> "$NODE_SENTINEL_LOG"
exit {exit}
"#
    )
    .into_bytes()
}

fn set_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn copy_fixture_file(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bun-install-lifecycle-runtime")
}

fn find_host_executable(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::var_os("PATH").ok_or("host PATH is missing")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("host executable {name} is missing").into())
}
