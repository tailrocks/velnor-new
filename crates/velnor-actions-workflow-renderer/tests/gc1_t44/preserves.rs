// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;

use velnor_actions_contract::{Step, StepRole};
use velnor_actions_workflow_renderer::preseed::{
    PRESEED_BUILD_OUTPUT, PRESEED_DOWNLOADED_BINARY, PreseedStageSource, check_preseed_step_order,
    preseed_manifest_step, preseed_manifest_verify_step, preseed_stage_step,
    source_bound_helper_digest,
};

use super::{
    build_argv, consumer_handoff, fail, manifest_digest_of, plan_handoff, require, shell_env,
    shell_role_step, shell_run, staged, target,
};

pub(super) fn check_digest_is_stable_identity() -> Result<(), Box<dyn Error>> {
    let first = source_bound_helper_digest(&build_argv(), target())?;
    let again = source_bound_helper_digest(&build_argv(), target())?;
    if first != again {
        return Err(fail("helper digest drifted between renders"));
    }
    if !velnor_actions_contract::ids::is_lower_hex_len(&first, 64) {
        return Err(fail("helper digest left its digest shape"));
    }
    Ok(())
}

pub(super) fn check_digest_follows_actual_inputs() -> Result<(), Box<dyn Error>> {
    let baseline = source_bound_helper_digest(&build_argv(), target())?;
    let mut changed_argv = build_argv();
    changed_argv.push("--frozen".to_owned());
    let changed_build = source_bound_helper_digest(&changed_argv, target())?;
    let changed_target = source_bound_helper_digest(&build_argv(), "aarch64-apple-darwin")?;
    let mut changed_toolchain = build_argv();
    "rust@1.90.0".clone_into(&mut changed_toolchain[5]);
    let changed_rust = source_bound_helper_digest(&changed_toolchain, target())?;
    for (label, digest) in [
        ("argv", changed_build),
        ("target", changed_target),
        ("toolchain", changed_rust),
    ] {
        if digest == baseline {
            return Err(fail(&format!("helper digest ignored its {label}")));
        }
        if !velnor_actions_contract::ids::is_lower_hex_len(&digest, 64) {
            return Err(fail(&format!("helper digest left its shape:{label}")));
        }
    }
    Ok(())
}

pub(super) fn check_manifest_binds_helper_context() -> Result<(), Box<dyn Error>> {
    let manifest = preseed_manifest_step(&build_argv(), target())?;
    if manifest.role != Some(StepRole::PreseedManifest) {
        return Err(fail("manifest left its application role"));
    }
    let env = shell_env(&manifest)?;
    if env.get("VELNOR_PRESEED_BINARY").map(String::as_str) != Some(PRESEED_BUILD_OUTPUT) {
        return Err(fail("manifest left its build output"));
    }
    if env.get("VELNOR_PRESEED_TARGET").map(String::as_str) != Some(target()) {
        return Err(fail("manifest left its target"));
    }
    if env.get("VELNOR_PRESEED_TOOLCHAIN").map(String::as_str) != Some("rust@1.91.0") {
        return Err(fail("manifest left its toolchain"));
    }
    let digest = manifest_digest_of(&manifest)?;
    let expected = source_bound_helper_digest(&build_argv(), target())?;
    if digest != expected {
        return Err(fail("manifest left its input identity"));
    }
    if !shell_run(&manifest)?
        .iter()
        .any(|arg| arg.contains(PRESEED_BUILD_OUTPUT))
    {
        return Err(fail("manifest left its build output command"));
    }
    Ok(())
}

pub(super) fn verify_script_of(step: &Step) -> Result<String, Box<dyn Error>> {
    shell_run(step)?
        .iter()
        .find(|arg| arg.contains("preseed-manifest.json"))
        .cloned()
        .ok_or_else(|| fail("verify left its manifest script"))
}

pub(super) fn staged_sha256(binary: &[u8]) -> Result<String, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("velnor-actions");
    std::fs::write(&path, binary)?;
    let output = Command::new("sha256sum").arg(&path).output()?;
    if !output.status.success() {
        return Err(fail("sha256sum left its digest"));
    }
    String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| fail("sha256sum left its shape"))
}

pub(super) fn run_verify_script(
    script: &str,
    manifest: &str,
    binary: &[u8],
    sha: &str,
) -> Result<(i32, String), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let stage = root.path().join("velnor").join("preseed");
    std::fs::create_dir_all(&stage)?;
    std::fs::write(stage.join("preseed-manifest.json"), manifest)?;
    std::fs::write(stage.join("velnor-actions"), binary)?;
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .env("RUNNER_TEMP", root.path())
        .env("GITHUB_SHA", sha)
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("verify left its exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined))
}

pub(super) fn check_verify_binds_input_digest() -> Result<(), Box<dyn Error>> {
    let manifest = preseed_manifest_step(&build_argv(), target())?;
    let digest = manifest_digest_of(&manifest)?.to_owned();
    let step = preseed_manifest_verify_step(&build_argv(), target())?;
    if step.role != Some(StepRole::PreseedVerifyManifest) {
        return Err(fail("verify left its application role"));
    }
    let script = verify_script_of(&step)?;
    require(&script, "input_digest", "verify digest field")?;
    require(&script, &digest, "verify digest value")?;
    let sha = "a".repeat(40);
    let binary = b"staged-preseed-helper";
    let sum = staged_sha256(binary)?;
    let manifest_text = format!(
        "{{\"schema\":1,\"commit\":\"{sha}\",\"target\":\"{target}\",\"toolchain\":\"rust@1.91.0\",\"sha256\":\"{sum}\",\"input_digest\":\"{digest}\"}}",
        target = target()
    );
    let (code, output) = run_verify_script(&script, &manifest_text, binary, &sha)?;
    if code != 0 {
        return Err(fail(&format!(
            "verify rejected its bound manifest:{output}"
        )));
    }
    let tampered = manifest_text.replacen(&digest, &"c".repeat(64), 1);
    let (code, _) = run_verify_script(&script, &tampered, binary, &sha)?;
    if code == 0 {
        return Err(fail("verify accepted its tampered digest"));
    }
    let missing = format!(
        "{{\"schema\":1,\"commit\":\"{sha}\",\"target\":\"{target}\",\"toolchain\":\"rust@1.91.0\",\"sha256\":\"{sum}\"}}",
        target = target()
    );
    let (code, _) = run_verify_script(&script, &missing, binary, &sha)?;
    if code == 0 {
        return Err(fail("verify accepted its missing digest"));
    }
    if preseed_manifest_verify_step(&build_argv(), "not-a-target").is_ok() {
        return Err(fail("verify accepted its bad target"));
    }
    Ok(())
}

pub(super) fn check_plan_runs_verified_handoff_first() -> Result<(), Box<dyn Error>> {
    let mut steps = plan_handoff()?;
    steps.insert(
        1,
        shell_role_step("Acquire Velnor", StepRole::AcquireVelnor),
    );
    check_preseed_step_order("velnor-plan", &steps, true)
        .map_err(|err| fail(&format!("verified handoff rejected:{err}")))?;
    check_preseed_step_order("velnor-plan", &plan_handoff()?, true)
        .map_err(|err| fail(&format!("plan handoff rejected:{err}")))?;
    Ok(())
}

pub(super) fn check_consumers_reuse_without_rebuild() -> Result<(), Box<dyn Error>> {
    for id in ["velnor-task", "crate-velnor-actions-cli"] {
        check_preseed_step_order(id, &consumer_handoff()?, false)
            .map_err(|err| fail(&format!("consumer reuse rejected:{id}:{err}")))?;
    }
    let stage = preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged())?;
    if stage.role != Some(StepRole::PreseedStage) {
        return Err(fail("consumer stage left its role"));
    }
    if !shell_run(&stage)?
        .iter()
        .any(|arg| arg.contains(PRESEED_DOWNLOADED_BINARY))
    {
        return Err(fail("consumer stage left its downloaded helper"));
    }
    Ok(())
}
