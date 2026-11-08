// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path};
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{finalized_jobs, prepare};

#[test]
fn enabled_runner_family_prepares_a_planned_job() -> Result<(), Box<dyn Error>> {
    let repo = super::maintenance_features::owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "default",
        "default",
    )?;
    let root = repo.path();
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["mise.toml", "mise.lock", "rust-toolchain.toml"] {
        fs::copy(project.join(name), root.join(name))?;
    }
    for name in [
        "scripts/generator-release/cargo-git-handoff.py",
        "scripts/generator-release/cargo-source-handoff.sh",
        "scripts/generator-release/source-qualification.sh",
    ] {
        let destination = root.join(name);
        fs::create_dir_all(destination.parent().ok_or("missing parent")?)?;
        fs::copy(project.join(name), destination)?;
    }
    fs::create_dir_all(root.join("scripts"))?;
    fs::write(
        root.join("scripts/check-runner-image-candidate.sh"),
        "#!/bin/sh\nexit 0\n",
    )?;
    let config_path = root.join(".velnor/config.toml");
    let mut config = fs::read_to_string(&config_path)?;
    config.push_str("\n[runner_image_family]\n");
    fs::write(config_path, config)?;
    let prepared = prepare(root)?;
    let jobs = finalized_jobs(&prepared)?;
    assert!(jobs.contains_key("image-runner-family-linux-amd64"));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn rendered_family_step_executes_bound_script_and_rejects_changed_source()
-> Result<(), Box<dyn Error>> {
    use std::process::Command;
    use velnor_actions_contract::StepKind;
    let repo = super::maintenance_features::owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "default",
        "default",
    )?;
    let root = repo.path().canonicalize()?;
    prepare_family_inputs(&root)?;
    let script_path = root.join("scripts/check-runner-image-candidate.sh");
    fs::write(
        &script_path,
        "#!/bin/sh\nprintf success > \"$GITHUB_WORKSPACE/family-sentinel\"\n",
    )?;
    let prepared = prepare(&root)?;
    let jobs = finalized_jobs(&prepared)?;
    let job = jobs
        .get("image-runner-family-linux-amd64")
        .ok_or("family job missing")?;
    let run_step = job
        .steps
        .iter()
        .find(|s| s.name == "Run runner image candidate")
        .ok_or("run step missing")?;
    let guard_step = job
        .steps
        .iter()
        .find(|s| s.name == "Verify runner family task source")
        .ok_or("guard missing")?;
    let StepKind::Shell { run, env } = &run_step.kind else {
        return Err("run is not shell".into());
    };
    let StepKind::Shell {
        run: guard,
        env: guard_env,
    } = &guard_step.kind
    else {
        return Err("guard is not shell".into());
    };
    let temp = tempfile::tempdir()?;
    let temp_root = temp.path().canonicalize()?;
    let bin = create_tool_fixture(&temp_root)?;
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let invoke = |argv: &[String],
                  variables: &std::collections::BTreeMap<String, String>|
     -> Result<std::process::Output, Box<dyn Error>> {
        let (program, args) = argv.split_first().ok_or("empty shell command")?;
        Ok(Command::new(program)
            .args(args)
            .envs(variables)
            .env("PATH", &path)
            .env("RUNNER_TEMP", &temp_root)
            .env("GITHUB_WORKSPACE", &root)
            .env("GITHUB_RUN_ID", "19")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .current_dir(&root)
            .output()?)
    };
    let guard_output = invoke(guard, guard_env)?;
    assert!(
        guard_output.status.success(),
        "guard: {}",
        String::from_utf8_lossy(&guard_output.stderr)
    );
    let output = invoke(run, env)?;
    assert!(
        output.status.success(),
        "run: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sentinel = root.join("family-sentinel");
    check_and_remove_sentinel(&sentinel)?;
    fs::write(
        &script_path,
        "#!/bin/sh\nprintf invalid > \"$GITHUB_WORKSPACE/family-sentinel\"\n",
    )?;
    assert!(!invoke(guard, guard_env)?.status.success());
    assert!(!sentinel.exists());
    Ok(())
}

#[cfg(target_os = "linux")]
fn create_tool_fixture(temp_root: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    let task = temp_root.join("velnor-verification-19-1-runner-image-family");
    let cargo = temp_root.join("velnor/cargo");
    let rustup = temp_root.join("velnor/rustup");
    let bin = temp_root.join("fake-bin");
    let mbx = task.join("data/installs/mr-boxington/1.29.2/mbx");
    let rustup_exe = cargo.join("bin/rustup");
    let toolchain = rustup.join("toolchains/1.99.1-x86_64-unknown-linux-gnu/bin");
    for dir in [
        task.join("home"),
        cargo.join("bin"),
        toolchain.clone(),
        bin.clone(),
        mbx.parent().ok_or("mbx parent")?.to_path_buf(),
    ] {
        fs::create_dir_all(dir)?;
    }
    for executable in [
        &mbx,
        &rustup_exe,
        &toolchain.join("rustc"),
        &toolchain.join("cargo"),
    ] {
        fs::write(executable, "#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(executable, fs::Permissions::from_mode(0o700))?;
    }
    let mise = bin.join("mise");
    fs::write(
        &mise,
        r#"#!/bin/bash
set -eu
case "$*" in
  *"config ls --json"*) printf '[]\n' ;;
  *"which mbx"*) printf '%s\n' "$MISE_DATA_DIR/installs/mr-boxington/1.29.2/mbx" ;;
  *"which rustc"*|*"which cargo"*) printf '%s\n' "$CARGO_HOME/bin/rustup" ;;
  *"rustup show active-toolchain"*) printf '%s (default)\n' "$VELNOR_EXPECTED_RUSTUP_TOOLCHAIN" ;;
  *"rustup which"*" rustc"*) printf '%s\n' "$RUSTUP_HOME/toolchains/$VELNOR_EXPECTED_RUSTUP_TOOLCHAIN/bin/rustc" ;;
  *"rustup which"*" cargo"*) printf '%s\n' "$RUSTUP_HOME/toolchains/$VELNOR_EXPECTED_RUSTUP_TOOLCHAIN/bin/cargo" ;;
  *"mr-boxington@"*" --version"*) printf 'mbx 1.29.2\n' ;;
  *"rustc --version"*) printf 'rustc 1.99.1 (fixture)\n' ;;
  *"cargo --version"*) printf 'cargo 1.99.1 (fixture)\n' ;;
  *" run --skip-tools "*) echo 'task not found' >&2; exit 1 ;;
  *" exec "*" -- bash scripts/check-runner-image-candidate.sh"*) exec bash scripts/check-runner-image-candidate.sh ;;
  *) printf 'unexpected mise command: %s\n' "$*" >&2; exit 1 ;;
esac
"#,
    )?;
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o700))?;
    let jq = bin.join("jq");
    fs::write(
        &jq,
        "#!/bin/sh\nif [ \"${1:-}\" = --version ]; then echo jq-1.8.0; else cat >/dev/null; printf '%s\\n' \"$GITHUB_WORKSPACE/mise.toml\"; fi\n",
    )?;
    fs::set_permissions(&jq, fs::Permissions::from_mode(0o700))?;
    Ok(bin)
}

#[cfg(target_os = "linux")]
fn prepare_family_inputs(root: &Path) -> Result<(), Box<dyn Error>> {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in ["mise.toml", "mise.lock", "rust-toolchain.toml"] {
        fs::copy(project.join(name), root.join(name))?;
    }
    for name in [
        "scripts/generator-release/cargo-git-handoff.py",
        "scripts/generator-release/cargo-source-handoff.sh",
        "scripts/generator-release/source-qualification.sh",
    ] {
        let destination = root.join(name);
        fs::create_dir_all(destination.parent().ok_or("missing parent")?)?;
        fs::copy(project.join(name), destination)?;
    }
    let config_path = root.join(".velnor/config.toml");
    let mut config = fs::read_to_string(&config_path)?;
    config.push_str("\n[runner_image_family]\n");
    fs::write(config_path, config)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn check_and_remove_sentinel(path: &Path) -> Result<(), Box<dyn Error>> {
    assert_eq!(fs::read(path)?, b"success");
    fs::remove_file(path)?;
    Ok(())
}
