// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_KEY_EXPR;
use velnor_actions_workflow_renderer::steps::{CompileDriver, MbxTransport, mbx_steps_for_driver};

use super::gc1_t53_publish::{github_output_value, shell_scratch};
use super::*;
fn run_discover_key(
    discover: &str,
    runner: &std::path::Path,
    workspace: &std::path::Path,
    scratch: &std::path::Path,
) -> Result<String, Box<dyn Error>> {
    let github_output = scratch.join("github-output");
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(discover)
        .env("RUNNER_TEMP", runner)
        .env("GITHUB_WORKSPACE", workspace)
        .env("GITHUB_OUTPUT", &github_output)
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
    github_output_value(&github_output, "key")
}

fn pack_over_ceiling_fixture(
    scratch: &std::path::Path,
    runner: &std::path::Path,
) -> Result<(), Box<dyn Error>> {
    // Pack a state root whose uncompressed bytes exceed the ceiling with
    // highly compressible content, so only an uncompressed cap can refuse
    // it after extraction.
    let pack_parent = scratch.join("pack");
    let pack_root = pack_parent.join("state");
    std::fs::create_dir_all(&pack_root)
        .map_err(|error| fail(&format!("pack root unwritable: {error}")))?;
    let chunk = vec![0u8; 1024 * 1024];
    let mut bulk = std::fs::File::create(pack_root.join("bulk"))
        .map_err(|error| fail(&format!("bulk unwritable: {error}")))?;
    for _ in 0..(OVER_CEILING_BYTES / 1024 / 1024) {
        bulk.write_all(&chunk)
            .map_err(|error| fail(&format!("bulk unwritable: {error}")))?;
    }
    drop(bulk);
    std::fs::create_dir_all(runner.join("velnor-gc1/transport/state"))
        .map_err(|error| fail(&format!("transport unwritable: {error}")))?;
    let packed = std::process::Command::new("tar")
        .arg("-czf")
        .arg(runner.join("velnor-gc1/transport/state/payload.tar.gz"))
        .arg("-C")
        .arg(&pack_parent)
        .arg("state")
        .status()
        .map_err(|error| fail(&format!("fixture pack failed to run: {error}")))?;
    if !packed.success() {
        return Err(fail("fixture pack failed"));
    }
    Ok(())
}

fn run_recover_cold(
    recover: &str,
    runner: &std::path::Path,
    workspace: &std::path::Path,
) -> Result<String, Box<dyn Error>> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(recover)
        .env("RUNNER_TEMP", runner)
        .env("GITHUB_WORKSPACE", workspace)
        .output()
        .map_err(|error| fail(&format!("recover failed to run: {error}")))?;
    if !output.status.success() {
        return Err(fail("over-ceiling archive must go cold, not fail the step"));
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_uncompressed_over_ceiling() -> Result<(), Box<dyn Error>> {
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
    let scratch = shell_scratch("recover-ceiling")?;
    let runner = scratch.join("runner");
    let workspace = scratch.join("workspace");
    std::fs::create_dir_all(runner.join("velnor"))
        .map_err(|error| fail(&format!("runner unwritable: {error}")))?;
    std::fs::create_dir_all(&workspace)
        .map_err(|error| fail(&format!("workspace unwritable: {error}")))?;
    std::fs::write(workspace.join("Cargo.lock"), "lock-fixture")
        .map_err(|error| fail(&format!("lock unwritable: {error}")))?;
    let discover = shell_script_of(&steps[3])?;
    let key = run_discover_key(&discover, &runner, &workspace, &scratch)?;
    pack_over_ceiling_fixture(&scratch, &runner)?;
    let recover = shell_script_of(&steps[5])?
        .replace(MBX_BUNDLE_KEY_EXPR, &key)
        .replace(
            "${{steps.mbx-bundle-restore.outputs.cache-matched-key}}",
            &key,
        );
    let combined = run_recover_cold(&recover, &runner, &workspace)?;
    require(&combined, "cache_corrupt", "recover ceiling")?;
    if runner.join("velnor/state-recovery.record").exists() {
        return Err(fail("over-ceiling archive must leave no recovery record"));
    }
    if runner.join("velnor-gc1/state").exists() {
        return Err(fail("over-ceiling archive must remove the state root"));
    }
    std::fs::remove_dir_all(&scratch).map_err(|error| fail(&format!("scratch stuck: {error}")))?;
    Ok(())
}
