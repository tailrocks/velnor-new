// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn shell_scratch(name: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!(
        "velnor-t30-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| fail(&format!("clock unreadable: {error}")))?
            .as_nanos()
    ));
    std::fs::create_dir_all(&root)
        .map_err(|error| fail(&format!("scratch unwritable: {error}")))?;
    Ok(root)
}

fn write_shimmed_executable(
    bin: &std::path::Path,
    name: &str,
    body: &str,
) -> Result<(), Box<dyn Error>> {
    std::fs::write(bin.join(name), body)
        .map_err(|error| fail(&format!("shim unwritable: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = std::fs::metadata(bin.join(name))
            .map_err(|error| fail(&format!("shim unreadable: {error}")))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(bin.join(name), permissions)
            .map_err(|error| fail(&format!("shim unexecutable: {error}")))?;
    }
    Ok(())
}

fn write_shimmed_path(scratch: &std::path::Path) -> Result<String, Box<dyn Error>> {
    let bin = scratch.join("shimbin");
    std::fs::create_dir_all(&bin)
        .map_err(|error| fail(&format!("shim dir unwritable: {error}")))?;
    write_shimmed_executable(&bin, "sha256sum", "#!/bin/sh\nexec shasum -a 256 \"$@\"\n")?;
    write_shimmed_executable(
        &bin,
        "mise",
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --) shift; break ;; *) shift ;; esac; done\nexec \"$@\"\n",
    )?;
    write_shimmed_executable(
        &bin,
        "mbx",
        "#!/bin/sh\necho \"mbx $*\" >> \"$MBX_STUB_LOG\"\ncase \"$1 $2\" in \"cache import\") [ -n \"$3\" ] && [ -f \"$3\" ] || exit 1 ;; *) exit 1 ;; esac\nexit 0\n",
    )?;
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_owned());
    Ok(format!("{}:{path}", bin.display()))
}

fn github_output_value(path: &std::path::Path, key: &str) -> Result<String, Box<dyn Error>> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| fail(&format!("github output unreadable: {error}")))?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix(&format!("{key}=")) {
            return Ok(value.to_owned());
        }
    }
    Err(fail(&format!("github output lost: {key}")))
}

struct BehaveSetup {
    scratch: std::path::PathBuf,
    runner: std::path::PathBuf,
    workspace: std::path::PathBuf,
    path_env: String,
    github_output: std::path::PathBuf,
    stub_log: std::path::PathBuf,
    discover: String,
    recover: String,
}

#[cfg(unix)]
fn behave_setup() -> Result<BehaveSetup, Box<dyn Error>> {
    let steps = transport_steps()?;
    let scratch = shell_scratch("behave")?;
    let runner = scratch.join("runner");
    let workspace = scratch.join("workspace");
    std::fs::create_dir_all(runner.join("velnor/mbx/actions"))
        .map_err(|error| fail(&format!("runner unwritable: {error}")))?;
    std::fs::create_dir_all(&workspace)
        .map_err(|error| fail(&format!("workspace unwritable: {error}")))?;
    std::fs::write(workspace.join("Cargo.lock"), "lock-fixture")
        .map_err(|error| fail(&format!("lock unwritable: {error}")))?;
    let path_env = write_shimmed_path(&scratch)?;
    Ok(BehaveSetup {
        github_output: scratch.join("github-output"),
        stub_log: scratch.join("stub-calls"),
        scratch,
        runner,
        workspace,
        path_env,
        discover: shell_script(&steps[3])?.to_owned(),
        recover: shell_script(&steps[5])?.to_owned(),
    })
}

#[cfg(unix)]
fn behave_discover(setup: &BehaveSetup) -> Result<(String, String), Box<dyn Error>> {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&setup.discover)
        .env("PATH", &setup.path_env)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("GITHUB_OUTPUT", &setup.github_output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("VELNOR_DEFAULT_REF", "refs/heads/main")
        .env("VELNOR_MBX_BUNDLE_VERSION", "1.2.3")
        .env("VELNOR_MBX_BUNDLE_TOOLCHAIN", "1.89.0")
        .env("VELNOR_MBX_BUNDLE_ROOT", ".")
        .status()
        .map_err(|error| fail(&format!("discover failed to run: {error}")))?;
    if !status.success() {
        return Err(fail("discover script failed on trusted push"));
    }
    let key = github_output_value(&setup.github_output, "key")?;
    let restore_prefix = github_output_value(&setup.github_output, "restore_keys")?;
    if github_output_value(&setup.github_output, "provenance")? != "trusted"
        || github_output_value(&setup.github_output, "decision")? != "admitted"
    {
        return Err(fail("discover misclassified a trusted push"));
    }
    Ok((key, restore_prefix))
}

#[cfg(unix)]
fn behave_fixture_payload(setup: &BehaveSetup) -> Result<(), Box<dyn Error>> {
    let pack_parent = setup.runner.join("velnor-gc1");
    let pack_root = pack_parent.join("state");
    std::fs::create_dir_all(&pack_root)
        .map_err(|error| fail(&format!("pack root unwritable: {error}")))?;
    std::fs::write(pack_root.join("payload"), "state bytes")
        .map_err(|error| fail(&format!("pack file unwritable: {error}")))?;
    std::fs::create_dir_all(pack_parent.join("transport/state"))
        .map_err(|error| fail(&format!("transport unwritable: {error}")))?;
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(
            setup
                .runner
                .join("velnor-gc1/transport/state/payload.tar.gz"),
        )
        .arg("-C")
        .arg(&pack_parent)
        .arg("state")
        .env("PATH", &setup.path_env)
        .status()
        .map_err(|error| fail(&format!("fixture pack failed to run: {error}")))?;
    if !status.success() {
        return Err(fail("fixture pack failed"));
    }
    Ok(())
}

#[cfg(unix)]
fn behave_run_recover(
    setup: &BehaveSetup,
    key: &str,
    matched: &str,
) -> Result<String, Box<dyn Error>> {
    let script = setup
        .recover
        .replace(MBX_BUNDLE_KEY_EXPR, key)
        .replace(MATCHED_KEY_BAKE, matched);
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&script)
        .env("PATH", &setup.path_env)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("MBX_CACHE_DIR", setup.runner.join("velnor/mbx"))
        .env("MBX_STUB_LOG", &setup.stub_log)
        .status()
        .map_err(|error| fail(&format!("recover failed to run: {error}")))?;
    if !status.success() {
        return Err(fail("recover script failed the step"));
    }
    std::fs::read_to_string(setup.runner.join("velnor/state-recovery.record"))
        .map_err(|error| fail(&format!("recovery record unreadable: {error}")))
}

#[cfg(unix)]
fn check_scripts_behave() -> Result<(), Box<dyn Error>> {
    let setup = behave_setup()?;
    let (key, restore_prefix) = behave_discover(&setup)?;
    behave_fixture_payload(&setup)?;

    let record = behave_run_recover(&setup, &key, &key)?;
    if !record.contains("decision=exact-hit") {
        return Err(fail("recover misclassified an exact hit"));
    }
    if !setup.runner.join("velnor-gc1/state/payload").is_file() {
        return Err(fail("recover lost the state root on exact hit"));
    }
    let calls = std::fs::read_to_string(&setup.stub_log)
        .map_err(|error| fail(&format!("stub log unreadable: {error}")))?;
    if !calls.contains("mbx cache import") {
        return Err(fail("recover skipped its store import"));
    }

    let fallback_matched = format!("{restore_prefix}oldsnap-0");
    let fallback_record = behave_run_recover(&setup, &key, &fallback_matched)?;
    if !fallback_record.contains("decision=predecessor-fallback") {
        return Err(fail("recover misclassified a predecessor fallback"));
    }

    let hostile_parent = setup.scratch.join("hostile");
    std::fs::create_dir_all(hostile_parent.join("state"))
        .map_err(|error| fail(&format!("hostile dir unwritable: {error}")))?;
    std::fs::create_dir_all(hostile_parent.join("tools"))
        .map_err(|error| fail(&format!("hostile dir unwritable: {error}")))?;
    std::fs::write(hostile_parent.join("state/ok"), "ok")
        .map_err(|error| fail(&format!("hostile file unwritable: {error}")))?;
    std::fs::write(hostile_parent.join("tools/sentinel"), "must never land")
        .map_err(|error| fail(&format!("sentinel unwritable: {error}")))?;
    let hostile_status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(
            setup
                .runner
                .join("velnor-gc1/transport/state/payload.tar.gz"),
        )
        .arg("-C")
        .arg(&hostile_parent)
        .arg("state")
        .arg("tools")
        .env("PATH", &setup.path_env)
        .status()
        .map_err(|error| fail(&format!("hostile pack failed to run: {error}")))?;
    if !hostile_status.success() {
        return Err(fail("hostile fixture pack failed"));
    }
    std::fs::remove_dir_all(setup.runner.join("velnor-gc1/state"))
        .map_err(|error| fail(&format!("fixture reset failed: {error}")))?;
    std::fs::remove_file(setup.runner.join("velnor/state-recovery.record"))
        .map_err(|error| fail(&format!("record reset failed: {error}")))?;
    let hostile_script = setup
        .recover
        .replace(MBX_BUNDLE_KEY_EXPR, &key)
        .replace(MATCHED_KEY_BAKE, &key);
    let hostile_run = std::process::Command::new("sh")
        .arg("-c")
        .arg(&hostile_script)
        .env("PATH", &setup.path_env)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("MBX_CACHE_DIR", setup.runner.join("velnor/mbx"))
        .env("MBX_STUB_LOG", &setup.stub_log)
        .status()
        .map_err(|error| fail(&format!("hostile recover failed to run: {error}")))?;
    if !hostile_run.success() {
        return Err(fail("hostile archive must go cold, not fail the step"));
    }
    if setup.runner.join("velnor-gc1/state").exists()
        || setup.runner.join("velnor/state-recovery.record").exists()
    {
        return Err(fail("hostile archive must not touch the state family"));
    }

    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t30_scripts_behave() -> Result<(), Box<dyn Error>> {
    check_scripts_behave()
}
