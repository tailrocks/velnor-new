use super::*;
use std::{fs, path::Path, process::Command};
use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::toolchain_env::credential_scrub;

use crate::git_fixture;

// Cargo's Git transport receives the shared sterile fixture environment too.
fn isolate_transport(command: &mut Command, home: &Path) {
    command.env_clear();
    let fixture = git_fixture::sterile_command();
    for (key, value) in fixture.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    command.env("HOME", home).env("GIT_TERMINAL_PROMPT", "0");
}

#[cfg(unix)]
#[test]
fn cargo_transport_discards_caller_git_authority_and_authentication() {
    let home = tempfile::tempdir().expect("owned transport home");
    let mut command = Command::new("sh");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_ASKPASS",
        "SSH_AUTH_SOCK",
        "GITHUB_TOKEN",
        "CARGO_REGISTRY_TOKEN",
    ] {
        command.env(key, "caller-canary");
    }
    isolate_transport(&mut command, home.path());
    checked(command.args(["-c", r#"test -z "${GIT_DIR+x}${GIT_WORK_TREE+x}${GIT_INDEX_FILE+x}${GIT_CONFIG_COUNT+x}${GIT_CONFIG_KEY_0+x}${GIT_CONFIG_VALUE_0+x}${GIT_ASKPASS+x}${SSH_AUTH_SOCK+x}${GITHUB_TOKEN+x}${CARGO_REGISTRY_TOKEN+x}" && test "$GIT_CONFIG_NOSYSTEM" = 1 && test "$GIT_CONFIG_GLOBAL" = /dev/null && test "$GIT_CONFIG_SYSTEM" = /dev/null && test -z "$GIT_TEMPLATE_DIR" && test "$GIT_TERMINAL_PROMPT" = 0"#]));
}

fn checked(command: &mut Command) -> String {
    let output = command.output().expect("execute fixture command");
    assert!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 output")
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    fs::write(path, bytes).expect("write fixture");
}

fn toolchain() -> std::path::PathBuf {
    let root = checked(Command::new("rustc").args(["--print", "sysroot"]));
    Path::new(root.trim()).join("bin")
}

fn git_dependency(root: &Path) -> String {
    write(
        &root.join("Cargo.toml"),
        "[package]\nname='local-dependency'\nversion='0.1.0'\nedition='2024'\n",
    );
    write(
        &root.join("src/lib.rs"),
        format!("// {}\npub fn value() -> u8 {{ 1 }}\n", root.display()),
    );
    checked(
        git_fixture::sterile_command()
            .current_dir(root)
            .args(["init", "-q"]),
    );
    for args in [vec!["add", "."], vec!["commit", "-qm", "fixture"]] {
        checked(
            git_fixture::command(root)
                .expect("fixture Git authority")
                .args(args),
        );
    }
    checked(
        git_fixture::command(root)
            .expect("fixture Git authority")
            .args(["rev-parse", "HEAD"]),
    )
    .trim()
    .to_owned()
}

fn workspace(root: &Path, name: &str, git: &Path, revision: &str) {
    write(
        &root.join("Cargo.toml"),
        format!(
            "[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n[workspace]\n[dependencies]\nlocal-dependency={{git='file://{}',rev='{revision}'}}\n",
            git.display()
        ),
    );
    write(
        &root.join("src/lib.rs"),
        "pub fn value() -> u8 { local_dependency::value() }\n",
    );
}

fn cargo(toolchain: &Path, clean: &Path, home: &Path, manifest: &Path, action: &str) -> Command {
    let mut command = Command::new(toolchain.join("cargo"));
    isolate_transport(&mut command, clean);
    command
        .current_dir(clean)
        .env("CARGO_HOME", home)
        .env("RUSTC", toolchain.join("rustc"))
        .args([action, "--manifest-path"])
        .arg(manifest);
    command
}

#[cfg(unix)]
fn mise_fixture(root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("mise");
    write(
        &path,
        format!(
            r#"#!/bin/sh
set -eu
test "$1" = --no-config
test "$2" = --no-env
test "$3" = --no-hooks
test "$4" = exec
test "$5" = rust@{}
test "$6" = --
test "$7" = cargo
test -z "${{GITHUB_TOKEN+x}}"
test -z "${{GH_TOKEN+x}}"
test -z "${{CARGO_REGISTRY_TOKEN+x}}"
test -z "${{NPM_TOKEN+x}}"
test -z "${{NODE_AUTH_TOKEN+x}}"
test "$CARGO_HOME" = "$MISE_CARGO_HOME"
test "$RUSTUP_HOME" = "$MISE_RUSTUP_HOME"
shift 7
printf '%s\n' "$1" >> "$VELNOR_TEST_CALLS"
exec "$VELNOR_TEST_CARGO" "$@"
"#,
            velnor_actions_mise::catalog::RUST_VERSION
        ),
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("executable wrapper");
}

#[cfg(unix)]
fn step_command(step: &Step, root: &Path, temp: &Path, tools: &Path, bin: &Path) -> Command {
    let StepKind::Shell { run, env } = &step.kind else {
        unreachable!("shell step")
    };
    let mut command = Command::new(&run[0]);
    isolate_transport(&mut command, temp);
    command
        .args(&run[1..])
        .current_dir(root)
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("RUNNER_TEMP", temp)
        .env("GITHUB_WORKSPACE", root)
        .env("RUSTC", tools.join("rustc"))
        .env("VELNOR_TEST_CARGO", tools.join("cargo"))
        .env("VELNOR_TEST_CALLS", temp.join("calls"))
        .env("GITHUB_TOKEN", "fixture-canary")
        .env("GH_TOKEN", "fixture-canary");
    for key in credential_scrub().keys() {
        command.env(key, "fixture-canary");
    }
    for (key, value) in env {
        command.env(
            key,
            value.replace("${{ runner.temp }}", temp.to_str().expect("temp path")),
        );
    }
    command
}

#[cfg(unix)]
fn run_step(step: &Step, root: &Path, temp: &Path, tools: &Path, bin: &Path) -> String {
    checked(&mut step_command(step, root, temp, tools, bin))
}

#[cfg(unix)]
fn verify_warm_sources(
    tools: &Path,
    clean: &Path,
    fresh: &Path,
    manifests: &[std::path::PathBuf],
    locks: &[Vec<u8>],
) {
    for (manifest, lock) in manifests.iter().zip(locks) {
        checked(cargo(tools, clean, fresh, manifest, "metadata").args([
            "--locked",
            "--offline",
            "--format-version",
            "1",
        ]));
        assert_eq!(
            &fs::read(manifest.with_file_name("Cargo.lock")).expect("lock after"),
            lock
        );
    }
}

#[cfg(unix)]
fn require_missing_sources(tools: &Path, clean: &Path, fresh: &Path, manifest: &Path) {
    let output = cargo(tools, clean, fresh, manifest, "metadata")
        .args(["--locked", "--offline", "--format-version", "1"])
        .output()
        .expect("cold metadata");
    assert!(
        !output.status.success(),
        "cold Git dependency must be absent"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("offline"));
}

#[cfg(unix)]
#[test]
fn cold_git_sources_prepare_all_workspaces_without_checkout_config_or_lock_changes() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let clean = temp.path().join("clean");
    fs::create_dir(&clean).expect("clean cwd");
    let git = temp.path().join("dependency");
    let revision = git_dependency(&git);
    let nested_git = temp.path().join("nested-dependency");
    let nested_revision = git_dependency(&nested_git);
    assert_ne!(revision, nested_revision);
    let root = temp.path().join("checkout");
    let nested = root.join("nested");
    workspace(&root, "root-package", &git, &revision);
    workspace(&nested, "nested-package", &nested_git, &nested_revision);
    let tools = toolchain();
    let manifests = [root.join("Cargo.toml"), nested.join("Cargo.toml")];
    let warm = temp.path().join("warm-cargo");
    for manifest in &manifests {
        checked(&mut cargo(
            &tools,
            &clean,
            &warm,
            manifest,
            "generate-lockfile",
        ));
    }
    let poison = root.join(".cargo/config.toml");
    write(&poison, "this is deliberately invalid TOML !!!\n");
    let locks: Vec<_> = manifests
        .iter()
        .map(|path| fs::read(path.with_file_name("Cargo.lock")).expect("lock"))
        .collect();
    let runner = temp.path().join("runner");
    fs::create_dir(&runner).expect("runner temp");
    let fresh = runner.join("velnor/cargo");
    for manifest in &manifests {
        require_missing_sources(&tools, &clean, &fresh, manifest);
    }
    let bin = temp.path().join("bin");
    mise_fixture(&bin);
    let (steps, env) = for_roots(
        &ToolCatalog::pinned(),
        &[String::new(), "nested".to_owned()],
    )
    .expect("P08 steps");
    assert_eq!(steps.len(), 2);
    assert_eq!(env["CARGO_HOME"], env["MISE_CARGO_HOME"]);
    assert_eq!(env["RUSTUP_HOME"], env["MISE_RUSTUP_HOME"]);
    run_step(&steps[0], &root, &runner, &tools, &bin);
    require_missing_sources(&tools, &clean, &fresh, &manifests[1]);
    run_step(&steps[1], &root, &runner, &tools, &bin);
    assert_eq!(
        fs::read_to_string(runner.join("calls"))
            .expect("calls")
            .lines()
            .filter(|line| *line == "fetch")
            .count(),
        2
    );
    verify_warm_sources(&tools, &clean, &fresh, &manifests, &locks);
    for step in &steps {
        assert!(run_step(step, &root, &runner, &tools, &bin).contains("sources hit"));
    }
    assert_eq!(
        fs::read_to_string(poison).expect("poison config unchanged"),
        "this is deliberately invalid TOML !!!\n"
    );
}

#[cfg(unix)]
#[test]
fn source_fetch_failure_propagates_without_lock_changes() {
    let temp = tempfile::tempdir().expect("owned fixture");
    let clean = temp.path().join("clean");
    fs::create_dir(&clean).expect("clean cwd");
    let git = temp.path().join("dependency");
    let revision = git_dependency(&git);
    let root = temp.path().join("checkout");
    workspace(&root, "root-package", &git, &revision);
    let tools = toolchain();
    checked(&mut cargo(
        &tools,
        &clean,
        &temp.path().join("warm"),
        &root.join("Cargo.toml"),
        "generate-lockfile",
    ));
    let lock = fs::read(root.join("Cargo.lock")).expect("lock");
    fs::remove_dir_all(git).expect("make locked source unavailable");
    let runner = temp.path().join("runner");
    fs::create_dir(&runner).expect("runner temp");
    let bin = temp.path().join("bin");
    mise_fixture(&bin);
    let (steps, _) = for_roots(&ToolCatalog::pinned(), &[String::new()]).expect("P08 step");
    let output = step_command(&steps[0], &root, &runner, &tools, &bin)
        .output()
        .expect("fetch failure");
    assert!(
        !output.status.success(),
        "source fetch failure must stop the job"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("sources miss"));
    assert_eq!(fs::read(root.join("Cargo.lock")).expect("lock after"), lock);
}
