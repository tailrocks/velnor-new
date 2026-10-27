// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_workflow_renderer::preseed::{
    PRESEED_MANIFEST_INPUT_DIGEST_ENV, PreseedStageSource, check_preseed_step_order,
    preseed_build_step, preseed_download_step, preseed_manifest_step, preseed_manifest_verify_step,
    preseed_stage_step, preseed_upload_step, preseed_verify_step,
};

mod preserves;
mod rejects;

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

#[test]
fn gc1_t44_preserves_contract() -> Result<(), Box<dyn Error>> {
    preserves::check_digest_is_stable_identity()?;
    preserves::check_digest_follows_actual_inputs()?;
    preserves::check_manifest_binds_helper_context()?;
    preserves::check_verify_binds_input_digest()?;
    preserves::check_plan_runs_verified_handoff_first()?;
    preserves::check_consumers_reuse_without_rebuild()?;
    Ok(())
}

#[test]
fn gc1_t44_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    rejects::check_rejects_unkeyed_helper()?;
    rejects::check_rejects_unverified_build_order()?;
    rejects::check_rejects_cache_cycle()?;
    rejects::check_rejects_context_crossing()?;
    Ok(())
}
