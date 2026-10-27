// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! COLD-path snapshot regressions (PR407 thread 4389237035).
//!
//! On a cold trusted push no state root was ever recovered, so the export
//! step must produce it with an opaque `mbx cache export` of the builds that
//! just ran under the export group; the recovery step must load the screened
//! payload into the local mbx store with `mbx cache import`. The real mbx
//! seam is stubbed through PATH so the tests stay hermetic and never touch a
//! developer store.

use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_KEY_EXPR;
use velnor_actions_workflow_renderer::steps::{CompileDriver, MbxTransport, mbx_steps_for_driver};

use super::gc1_t53_publish::{github_output_value, shell_scratch};
use super::*;

struct ColdSetup {
    scratch: std::path::PathBuf,
    runner: std::path::PathBuf,
    workspace: std::path::PathBuf,
    github_output: std::path::PathBuf,
    stub_log: std::path::PathBuf,
    bin: std::path::PathBuf,
    export: String,
    record: String,
    recover: String,
}

fn write_executable(path: &std::path::Path, body: &str) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, body).map_err(|error| fail(&format!("stub unwritable: {error}")))?;
    let mut permissions = std::fs::metadata(path)
        .map_err(|error| fail(&format!("stub unreadable: {error}")))?
        .permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(path, permissions)
        .map_err(|error| fail(&format!("stub unexecutable: {error}")))?;
    Ok(())
}

fn cold_setup(name: &str) -> Result<ColdSetup, Box<dyn Error>> {
    let scratch = shell_scratch(name)?;
    let runner = scratch.join("runner");
    let workspace = scratch.join("workspace");
    let bin = scratch.join("bin");
    std::fs::create_dir_all(runner.join("velnor/mbx/actions"))
        .map_err(|error| fail(&format!("runner unwritable: {error}")))?;
    std::fs::create_dir_all(&workspace)
        .map_err(|error| fail(&format!("workspace unwritable: {error}")))?;
    std::fs::create_dir_all(&bin).map_err(|error| fail(&format!("bin unwritable: {error}")))?;
    std::fs::write(workspace.join("Cargo.lock"), "lock-fixture")
        .map_err(|error| fail(&format!("lock unwritable: {error}")))?;
    write_executable(
        &bin.join("mise"),
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --) shift; break ;; *) shift ;; esac; done\nexec \"$@\"\n",
    )?;
    write_executable(
        &bin.join("mbx"),
        "#!/bin/sh\necho \"mbx $*\" >> \"$MBX_STUB_LOG\"\n[ \"${MBX_STUB_FAIL:-}\" = \"$2\" ] && exit 1\ncase \"$1 $2\" in \"cache export\") dir=\"\"; for last in \"$@\"; do dir=\"$last\"; done; mkdir -p \"$dir\" || exit 1; printf 'cold built state\\n' > \"$dir/payload\" || exit 1 ;; \"cache import\") [ -n \"$3\" ] && [ -f \"$3\" ] || exit 1 ;; *) exit 1 ;; esac\nexit 0\n",
    )?;
    let text = render_text()?;
    let export = extract_run(step_section(&text, EXPORT_STEP)?, "export")?;
    let record = extract_run(step_section(&text, RECORD_STEP)?, "record")?;
    let steps = mbx_steps_for_driver(
        &format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
        CompileDriver::Mbx,
        "1.2.3",
        "1.89.0",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )?
    .ok_or_else(|| fail("mbx driver produced no steps"))?;
    let recover = shell_script_of(&steps[5])?;
    Ok(ColdSetup {
        github_output: scratch.join("github-output"),
        stub_log: scratch.join("stub-calls"),
        scratch,
        runner,
        workspace,
        bin,
        export,
        record,
        recover,
    })
}

fn stub_path(setup: &ColdSetup) -> Result<String, Box<dyn Error>> {
    let path = std::env::var("PATH").map_err(|error| fail(&format!("path unreadable: {error}")))?;
    Ok(format!("{}:{path}", setup.bin.display()))
}

/// Run the discovery prelude so the export/recover scripts see a trusted
/// discovery record with a matching compat digest, then record the producer
/// success exactly like the producer-mark step does.
fn cold_discover(setup: &ColdSetup) -> Result<(), Box<dyn Error>> {
    let steps = mbx_steps_for_driver(
        &format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
        CompileDriver::Mbx,
        "1.2.3",
        "1.89.0",
        driver_env()?,
        MbxTransport::SeedRead,
        None,
    )?
    .ok_or_else(|| fail("mbx driver produced no steps"))?;
    let discover = shell_script_of(&steps[3])?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&discover)
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
    if github_output_value(&setup.github_output, "provenance")? != "trusted" {
        return Err(fail("discover misclassified a trusted push"));
    }
    std::fs::write(setup.runner.join("velnor/state-producer-ok"), "")
        .map_err(|error| fail(&format!("mark unwritable: {error}")))?;
    Ok(())
}

fn mbx_env(setup: &ColdSetup, command: &mut std::process::Command) -> Result<(), Box<dyn Error>> {
    let path = stub_path(setup)?;
    command
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .env("GITHUB_OUTPUT", &setup.github_output)
        .env("MBX_CACHE_EXPORT_GROUP", "velnor")
        .env("MBX_CACHE_DIR", setup.runner.join("velnor/mbx"))
        .env("MBX_STUB_LOG", &setup.stub_log)
        .env("PATH", path);
    Ok(())
}

fn stub_calls(setup: &ColdSetup) -> Result<String, Box<dyn Error>> {
    std::fs::read_to_string(&setup.stub_log)
        .map_err(|error| fail(&format!("stub log unreadable: {error}")))
}

#[test]
#[cfg(unix)]
fn gc1_t53_cold_trusted_push_publishes_first_entry() -> Result<(), Box<dyn Error>> {
    let setup = cold_setup("cold-first")?;
    cold_discover(&setup)?;
    // Cold: no state root was ever recovered.
    if setup.runner.join("velnor-gc1/state").exists() {
        return Err(fail("cold fixture leaked a state root"));
    }
    let mut export = std::process::Command::new("sh");
    export.arg("-c").arg(&setup.export);
    mbx_env(&setup, &mut export)?;
    let output = export
        .output()
        .map_err(|error| fail(&format!("export failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail(&format!(
            "export script failed the step: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    require(&combined, "readback verified", "cold export")?;
    forbid(&combined, "state snapshot published (", "cold export")?;
    if github_output_value(&setup.github_output, "exported")? != "true" {
        return Err(fail("cold export must admit the produced snapshot"));
    }
    if !setup
        .runner
        .join("velnor/state-publication.staged")
        .is_file()
    {
        return Err(fail("cold export must stage its attestation"));
    }
    if setup
        .runner
        .join("velnor/state-publication.record")
        .exists()
    {
        return Err(fail("cold export must not write the publication record"));
    }
    require(&stub_calls(&setup)?, "mbx cache export", "cold export")?;
    let recorded = std::process::Command::new("sh")
        .arg("-c")
        .arg(&setup.record)
        .env("RUNNER_TEMP", &setup.runner)
        .env("GITHUB_WORKSPACE", &setup.workspace)
        .output()
        .map_err(|error| fail(&format!("record failed to run: {error}")))?;
    if !recorded.status.success() {
        return Err(fail("record script failed the step"));
    }
    require(
        &format!(
            "{}{}",
            String::from_utf8_lossy(&recorded.stdout),
            String::from_utf8_lossy(&recorded.stderr)
        ),
        "state snapshot published (",
        "cold record",
    )?;
    if !setup
        .runner
        .join("velnor/state-publication.record")
        .is_file()
    {
        return Err(fail("record must write the publication record"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t53_cold_export_failure_skips_clean() -> Result<(), Box<dyn Error>> {
    let setup = cold_setup("cold-export-fail")?;
    cold_discover(&setup)?;
    let mut export = std::process::Command::new("sh");
    export.arg("-c").arg(&setup.export);
    mbx_env(&setup, &mut export)?;
    export.env("MBX_STUB_FAIL", "export");
    let output = export
        .output()
        .map_err(|error| fail(&format!("export failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail("failed export must skip, not fail the step"));
    }
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    require(&combined, "not published (export_failed)", "cold export")?;
    forbid(&combined, "state snapshot published (", "cold export")?;
    if setup
        .runner
        .join("velnor-gc1/transport/state/payload.tar.gz")
        .exists()
    {
        return Err(fail("failed export must leave no payload"));
    }
    if setup
        .runner
        .join("velnor/state-publication.staged")
        .exists()
        || setup
            .runner
            .join("velnor/state-publication.record")
            .exists()
    {
        return Err(fail("failed export must leave no evidence"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

fn pack_valid_fixture(setup: &ColdSetup, key: &str) -> Result<String, Box<dyn Error>> {
    let pack_parent = setup.scratch.join("pack");
    let pack_root = pack_parent.join("state");
    std::fs::create_dir_all(&pack_root)
        .map_err(|error| fail(&format!("pack root unwritable: {error}")))?;
    std::fs::write(pack_root.join("payload"), "recovered state")
        .map_err(|error| fail(&format!("pack file unwritable: {error}")))?;
    std::fs::create_dir_all(setup.runner.join("velnor-gc1/transport/state"))
        .map_err(|error| fail(&format!("transport unwritable: {error}")))?;
    let packed = std::process::Command::new("tar")
        .arg("-czf")
        .arg(
            setup
                .runner
                .join("velnor-gc1/transport/state/payload.tar.gz"),
        )
        .arg("-C")
        .arg(&pack_parent)
        .arg("state")
        .status()
        .map_err(|error| fail(&format!("fixture pack failed to run: {error}")))?;
    if !packed.success() {
        return Err(fail("fixture pack failed"));
    }
    Ok(setup.recover.replace(MBX_BUNDLE_KEY_EXPR, key).replace(
        "${{steps.mbx-bundle-restore.outputs.cache-matched-key}}",
        key,
    ))
}

fn cold_discover_key(setup: &ColdSetup) -> Result<String, Box<dyn Error>> {
    cold_discover(setup)?;
    github_output_value(&setup.github_output, "key")
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_invokes_mbx_import() -> Result<(), Box<dyn Error>> {
    let setup = cold_setup("cold-recover-import")?;
    let key = cold_discover_key(&setup)?;
    let recover = pack_valid_fixture(&setup, &key)?;
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg(&recover);
    mbx_env(&setup, &mut command)?;
    let output = command
        .output()
        .map_err(|error| fail(&format!("recover failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail(&format!(
            "recover script failed the step: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    require(
        &format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
        "state snapshot recovered (decision=",
        "recover import",
    )?;
    if !setup.runner.join("velnor/state-recovery.record").is_file() {
        return Err(fail("recover must write the recovery record"));
    }
    if !setup.runner.join("velnor-gc1/state").is_dir() {
        return Err(fail("recover must materialize the state root"));
    }
    require(&stub_calls(&setup)?, "mbx cache import", "recover import")?;
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_import_failure_goes_cold() -> Result<(), Box<dyn Error>> {
    let setup = cold_setup("cold-recover-fail")?;
    let key = cold_discover_key(&setup)?;
    let recover = pack_valid_fixture(&setup, &key)?;
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg(&recover);
    mbx_env(&setup, &mut command)?;
    command.env("MBX_STUB_FAIL", "import");
    let output = command
        .output()
        .map_err(|error| fail(&format!("recover failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail("failed import must go cold, not fail the step"));
    }
    require(
        &format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
        "cache_corrupt",
        "recover import",
    )?;
    if setup.runner.join("velnor/state-recovery.record").exists() {
        return Err(fail("failed import must leave no recovery record"));
    }
    if setup.runner.join("velnor-gc1/state").exists() {
        return Err(fail("failed import must remove the state root"));
    }
    std::fs::remove_dir_all(&setup.scratch)
        .map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}
