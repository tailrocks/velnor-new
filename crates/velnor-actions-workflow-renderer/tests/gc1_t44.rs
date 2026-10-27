// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::process::Command;

use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_workflow_renderer::preseed::{
    PRESEED_BUILD_OUTPUT, PRESEED_DOWNLOADED_BINARY, PRESEED_MANIFEST_INPUT_DIGEST_ENV,
    PreseedStageSource, check_preseed_step_order, preseed_build_step, preseed_download_step,
    preseed_manifest_step, preseed_manifest_verify_step, preseed_stage_step, preseed_upload_step,
    preseed_verify_step, source_bound_helper_digest,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn build_argv() -> Vec<String> {
    [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.91.0",
        "--",
        "mbx",
        "build",
        "--release",
        "--locked",
        "--package",
        "velnor-actions-cli",
        "--bin",
        "velnor-actions",
    ]
    .iter()
    .map(|value| (*value).to_owned())
    .collect()
}

fn probe_argv() -> Vec<String> {
    [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.91.0",
        "--",
        "mbx",
        "--version",
    ]
    .iter()
    .map(|value| (*value).to_owned())
    .collect()
}

fn target() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

fn staged() -> String {
    "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.55".to_owned()
}

fn shell_role_step(name: &str, role: StepRole) -> Step {
    let mut step = Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
            env: BTreeMap::new(),
        },
    };
    step.role = Some(role);
    step
}

fn shell_env(step: &Step) -> Result<&BTreeMap<String, String>, Box<dyn Error>> {
    match &step.kind {
        StepKind::Shell { env, .. } => Ok(env),
        _ => Err(fail("expected a shell step")),
    }
}

fn shell_run(step: &Step) -> Result<&Vec<String>, Box<dyn Error>> {
    match &step.kind {
        StepKind::Shell { run, .. } => Ok(run),
        _ => Err(fail("expected a shell step")),
    }
}

fn manifest_digest_of(step: &Step) -> Result<&str, Box<dyn Error>> {
    shell_env(step)?
        .get(PRESEED_MANIFEST_INPUT_DIGEST_ENV)
        .map(String::as_str)
        .ok_or_else(|| fail("manifest left its input digest"))
}

fn plan_handoff() -> Result<Vec<Step>, Box<dyn Error>> {
    Ok(vec![
        shell_role_step("Prepare pinned tools", StepRole::PreparePinnedTools),
        preseed_build_step(&build_argv(), &BTreeMap::new())?,
        preseed_verify_step(&probe_argv(), "1.9.0", &BTreeMap::new())?,
        preseed_manifest_step(&build_argv(), target())?,
        preseed_upload_step()?,
        preseed_stage_step(PreseedStageSource::LocalBuild, &staged())?,
    ])
}

fn consumer_handoff() -> Result<Vec<Step>, Box<dyn Error>> {
    Ok(vec![
        preseed_download_step()?,
        preseed_manifest_verify_step(&build_argv(), target())?,
        preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged())?,
    ])
}

fn expect_order_error(
    id: &str,
    steps: &[Step],
    is_plan: bool,
    problem: &str,
) -> Result<(), Box<dyn Error>> {
    match check_preseed_step_order(id, steps, is_plan) {
        Ok(()) => Err(fail(&format!("accepted:{problem}"))),
        Err(err) => require(&err.to_string(), problem, "rejection"),
    }
}

fn check_digest_is_stable_identity() -> Result<(), Box<dyn Error>> {
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

fn check_digest_follows_actual_inputs() -> Result<(), Box<dyn Error>> {
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

fn check_manifest_binds_helper_context() -> Result<(), Box<dyn Error>> {
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

fn verify_script_of(step: &Step) -> Result<String, Box<dyn Error>> {
    shell_run(step)?
        .iter()
        .find(|arg| arg.contains("preseed-manifest.json"))
        .cloned()
        .ok_or_else(|| fail("verify left its manifest script"))
}

fn staged_sha256(binary: &[u8]) -> Result<String, Box<dyn Error>> {
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

fn run_verify_script(
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

fn check_verify_binds_input_digest() -> Result<(), Box<dyn Error>> {
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

fn check_plan_runs_verified_handoff_first() -> Result<(), Box<dyn Error>> {
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

fn check_consumers_reuse_without_rebuild() -> Result<(), Box<dyn Error>> {
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

#[test]
fn gc1_t44_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_digest_is_stable_identity()?;
    check_digest_follows_actual_inputs()?;
    check_manifest_binds_helper_context()?;
    check_verify_binds_input_digest()?;
    check_plan_runs_verified_handoff_first()?;
    check_consumers_reuse_without_rebuild()?;
    Ok(())
}

fn check_rejects_unkeyed_helper() -> Result<(), Box<dyn Error>> {
    if source_bound_helper_digest(&build_argv(), "not-a-target").is_ok() {
        return Err(fail("helper digest accepted its bad target"));
    }
    if source_bound_helper_digest(&["mbx".to_owned(), "build".to_owned()], target()).is_ok() {
        return Err(fail("helper digest accepted its bad build"));
    }
    if preseed_manifest_step(&build_argv(), "not-a-target").is_ok() {
        return Err(fail("manifest accepted its bad target"));
    }
    let mut unkeyed = plan_handoff()?;
    let manifest_at = unkeyed
        .iter()
        .position(|step| step.role == Some(StepRole::PreseedManifest))
        .ok_or_else(|| fail("handoff left its manifest"))?;
    match &mut unkeyed[manifest_at].kind {
        StepKind::Shell { env, .. } => {
            env.remove(PRESEED_MANIFEST_INPUT_DIGEST_ENV);
        }
        _ => return Err(fail("manifest left its shell shape")),
    }
    expect_order_error(
        "velnor-plan",
        &unkeyed,
        true,
        "preseed_manifest_without_input_digest",
    )?;
    let mut malformed = plan_handoff()?;
    let manifest_at = malformed
        .iter()
        .position(|step| step.role == Some(StepRole::PreseedManifest))
        .ok_or_else(|| fail("handoff left its manifest"))?;
    match &mut malformed[manifest_at].kind {
        StepKind::Shell { env, .. } => {
            env.insert(
                PRESEED_MANIFEST_INPUT_DIGEST_ENV.to_owned(),
                "not-a-digest".to_owned(),
            );
        }
        _ => return Err(fail("manifest left its shell shape")),
    }
    expect_order_error(
        "velnor-plan",
        &malformed,
        true,
        "preseed_manifest_without_input_digest",
    )?;
    Ok(())
}

fn check_rejects_unverified_build_order() -> Result<(), Box<dyn Error>> {
    let mut late_restore = plan_handoff()?;
    late_restore.remove(0);
    expect_order_error(
        "velnor-plan",
        &late_restore,
        true,
        "preseed_build_without_restore",
    )?;
    let mut late_controller = plan_handoff()?;
    late_controller.insert(
        1,
        shell_role_step("Acquire Velnor", StepRole::AcquireVelnor),
    );
    let acquire = late_controller.remove(1);
    late_controller.push(acquire);
    expect_order_error(
        "velnor-plan",
        &late_controller,
        true,
        "preseed_controller_after_build",
    )?;
    Ok(())
}

fn check_rejects_cache_cycle() -> Result<(), Box<dyn Error>> {
    let mut save_inside = plan_handoff()?;
    save_inside.insert(2, shell_role_step("Save tools", StepRole::ToolsCacheSave));
    expect_order_error("velnor-plan", &save_inside, true, "preseed_cache_cycle")?;
    let mut restore_inside = plan_handoff()?;
    restore_inside.insert(
        3,
        shell_role_step("Restore sources", StepRole::CargoSourcesRestore),
    );
    expect_order_error("velnor-plan", &restore_inside, true, "preseed_cache_cycle")?;
    let mut consumer_cycle = consumer_handoff()?;
    consumer_cycle.insert(1, shell_role_step("Save tools", StepRole::ToolsCacheSave));
    expect_order_error(
        "crate-velnor-actions-cli",
        &consumer_cycle,
        false,
        "preseed_cache_cycle",
    )?;
    Ok(())
}

fn check_rejects_context_crossing() -> Result<(), Box<dyn Error>> {
    let mut consumer_builds = consumer_handoff()?;
    consumer_builds.insert(1, preseed_build_step(&build_argv(), &BTreeMap::new())?);
    expect_order_error(
        "crate-velnor-actions-cli",
        &consumer_builds,
        false,
        "preseed_rebuild_in_consumer",
    )?;
    let mut consumer_compiles = consumer_handoff()?;
    consumer_compiles.push(preseed_stage_step(
        PreseedStageSource::LocalBuild,
        &staged(),
    )?);
    expect_order_error(
        "crate-velnor-actions-cli",
        &consumer_compiles,
        false,
        "preseed_stage_context",
    )?;
    let mut plan_downloads = plan_handoff()?;
    let stage_at = plan_downloads
        .iter()
        .position(|step| step.role == Some(StepRole::PreseedStage))
        .ok_or_else(|| fail("handoff left its stage"))?;
    plan_downloads[stage_at] =
        preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged())?;
    expect_order_error(
        "velnor-plan",
        &plan_downloads,
        true,
        "preseed_stage_context",
    )?;
    Ok(())
}

#[test]
fn gc1_t44_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_unkeyed_helper()?;
    check_rejects_unverified_build_order()?;
    check_rejects_cache_cycle()?;
    check_rejects_context_crossing()?;
    Ok(())
}
