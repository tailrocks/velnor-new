//! Pre-seed mode: build-once templates plus strict closure gates.
use std::collections::BTreeMap;

use velnor_actions_contract::Job;
use velnor_actions_workflow_renderer::{
    INTERNAL_OP_ENV, PRESEED_BUILD_OUTPUT, PRESEED_MANIFEST_BINARY_ENV, PRESEED_MANIFEST_OUT_ENV,
    PRESEED_MANIFEST_TARGET_ENV, PRESEED_MANIFEST_TOOLCHAIN_ENV, PreseedStageSource, RenderError,
    WRITE_PRESEED_MANIFEST_OPERATION, checkout_step, merge_step, plan_step, preseed_build_step,
    preseed_download_step, preseed_manifest_step, preseed_manifest_verify_step, preseed_stage_step,
    preseed_upload_step, preseed_verify_step,
};

use super::impl_renderer_fixtures::*;

const TARGET: &str = "x86_64-unknown-linux-gnu";
const MARK: &str = "(pre-seed trust-on-review)";
const MBX_VERSION: &str = "1.21.1";

fn mbx_probe() -> Vec<String> {
    mise_argv("rust@1.98.1", "mbx", &["--version"])
}

fn build_argv() -> Vec<String> {
    mise_argv(
        "rust@1.98.1",
        "mbx",
        &[
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ],
    )
}

pub(crate) fn preseed_plan() -> Result<(String, Job), RenderError> {
    let build = build_argv();
    Ok(job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            preseed_build_step(&build, &BTreeMap::new())?,
            preseed_verify_step(&mbx_probe(), MBX_VERSION, &BTreeMap::new())?,
            preseed_manifest_step(&build, TARGET)?,
            preseed_upload_step()?,
            preseed_stage_step(PreseedStageSource::LocalBuild, STAGED)?,
            plan_step(),
        ],
    ))
}

pub(crate) fn preseed_final() -> Result<(String, Job), RenderError> {
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            preseed_download_step()?,
            preseed_manifest_verify_step(TARGET)?,
            preseed_stage_step(PreseedStageSource::DownloadedArtifact, STAGED)?,
            merge_step(),
        ],
    );
    final_job.1.condition = Some("always()".to_owned());
    Ok(final_job)
}

#[test]
fn preseed_templates_carry_trust_mark_and_exact_artifact() -> Result<(), RenderError> {
    let build = build_argv();
    let manifest = preseed_manifest_step(&build, TARGET)?;
    let build_step = preseed_build_step(
        &build,
        &BTreeMap::from([("MISE_CARGO_HOME".to_owned(), "owned".to_owned())]),
    )?;
    // S1: the helper build compiles PR source, so it unsets runner
    // credentials before exec and scrubs the step env.
    let velnor_actions_contract::StepKind::Shell { run, env } = &build_step.kind else {
        panic!("pre-seed build must be shell: {:?}", build_step.kind);
    };
    assert_eq!(run.first().map(String::as_str), Some("env"));
    assert!(run.contains(&"GITHUB_TOKEN".to_owned()), "{run:?}");
    for key in velnor_actions_workflow_renderer::toolchain_env::STEP_CREDENTIAL_DENYLIST {
        assert_eq!(
            env.get(key).map(String::as_str),
            Some(""),
            "pre-seed build must scrub {key}"
        );
    }
    let home = env.get("MISE_CARGO_HOME").map(String::as_str);
    assert_eq!(home, Some("owned"), "build carries caller homes");
    for step in [
        build_step,
        preseed_verify_step(&mbx_probe(), MBX_VERSION, &BTreeMap::new())?,
        manifest,
        preseed_upload_step()?,
        preseed_download_step()?,
        preseed_manifest_verify_step(TARGET)?,
        preseed_stage_step(PreseedStageSource::LocalBuild, STAGED)?,
    ] {
        assert!(
            step.name.ends_with(MARK),
            "unmarked pre-seed step: {:?}",
            step.name
        );
    }
    assert!(
        preseed_manifest_step(&build, "wasm32-unknown-unknown").is_err(),
        "bad target accepted"
    );
    assert!(
        preseed_manifest_step(&["mbx".to_owned()], TARGET).is_err(),
        "toolchain-less build accepted"
    );
    assert!(
        preseed_stage_step(PreseedStageSource::LocalBuild, "target/release/x").is_err(),
        "unstaged path accepted"
    );
    Ok(())
}

#[test]
fn preseed_manifest_step_runs_fresh_binary_with_op() -> Result<(), RenderError> {
    let step = preseed_manifest_step(&build_argv(), TARGET)?;
    let velnor_actions_contract::StepKind::Shell { run, env } = &step.kind else {
        panic!("manifest must be a shell step");
    };
    // Fresh binary, no sh -c wrapper: just the constructor's `env -u`
    // unset prefix ahead of the payload.
    assert_eq!(
        run.last().map(String::as_str),
        Some(PRESEED_BUILD_OUTPUT),
        "fresh binary payload: {run:?}"
    );
    assert_eq!(
        run.first().map(String::as_str),
        Some("env"),
        "unset prefix head: {run:?}"
    );
    assert_eq!(
        env.get(INTERNAL_OP_ENV).map(String::as_str),
        Some(WRITE_PRESEED_MANIFEST_OPERATION),
        "manifest op selected: {env:?}"
    );
    assert_eq!(
        env.get(PRESEED_MANIFEST_BINARY_ENV).map(String::as_str),
        Some(PRESEED_BUILD_OUTPUT),
        "binary path: {env:?}"
    );
    assert_eq!(
        env.get(PRESEED_MANIFEST_OUT_ENV).map(String::as_str),
        Some("${{ runner.temp }}/velnor/preseed-output"),
        "expression-form out dir, no shell expansion: {env:?}"
    );
    assert_eq!(
        env.get(PRESEED_MANIFEST_TARGET_ENV).map(String::as_str),
        Some(TARGET),
        "literal target: {env:?}"
    );
    assert_eq!(
        env.get(PRESEED_MANIFEST_TOOLCHAIN_ENV).map(String::as_str),
        Some("rust@1.98.1"),
        "toolchain derived from the build vector: {env:?}"
    );
    for value in env.values() {
        assert!(
            !value.contains("printf") && !value.contains("sha256sum"),
            "no shell-composed JSON or digest: {value}"
        );
    }
    Ok(())
}

#[test]
fn preseed_verify_pins_binary_and_mbx_route() -> Result<(), RenderError> {
    let step = preseed_verify_step(
        &mbx_probe(),
        MBX_VERSION,
        &BTreeMap::from([("MISE_CARGO_HOME".to_owned(), "owned".to_owned())]),
    )?;
    let velnor_actions_contract::StepKind::Shell { run, env } = &step.kind else {
        panic!("verify must be a shell step");
    };
    let home = env.get("MISE_CARGO_HOME").map(String::as_str);
    assert_eq!(home, Some("owned"), "verify carries caller homes");
    assert_eq!((run[0].as_str(), run[1].as_str()), ("sh", "-c"));
    for need in [
        "test -x target/release/velnor-actions",
        "mise --no-config --no-env --no-hooks exec rust@1.98.1 -- mbx --version",
        "mkdir -p \"$RUNNER_TEMP/velnor\"",
        "mise --no-config --no-env --no-hooks exec rust@1.98.1 -- mbx --version > \"$RUNNER_TEMP/velnor/preseed-mbx-version\"",
        "grep -qxF \"mbx 1.21.1\" \"$RUNNER_TEMP/velnor/preseed-mbx-version\"",
    ] {
        assert!(run[2].contains(need), "verify misses {need}: {}", run[2]);
    }
    assert!(
        !run[2].contains('\'') && !run[2].contains('`') && !run[2].contains('|'),
        "verify has no unneeded quoting or status-masking pipeline: {}",
        run[2]
    );
    for bad_version in ["", "latest", "1.19", "v1.19.0", "1.19.0 "] {
        assert!(
            preseed_verify_step(&mbx_probe(), bad_version, &BTreeMap::new()).is_err(),
            "version accepted: {bad_version}"
        );
    }
    for bad_probe in [
        Vec::new(),
        vec!["mbx".to_owned(), "--version".to_owned()],
        mise_argv("rust@1.98.1", "mbx", &["--help"]),
        mise_argv("mr-boxington@1.21.1", "mbx", &["--version"]),
        mise_argv("rust@1.98.1", "mbx", &["--version", "--verbose"]),
    ] {
        assert!(
            preseed_verify_step(&bad_probe, MBX_VERSION, &BTreeMap::new()).is_err(),
            "probe accepted: {bad_probe:?}"
        );
    }
    Ok(())
}

#[test]
fn preseed_requires_velnor_policy() {
    let mut ctx = fixture_ctx();
    ctx.preseed = true;
    let err = strict(&fixture_ir(vec![minimal_plan_job().expect("plan")]), &ctx);
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_requires_velnor_policy")),
        "consumer pre-seed accepted"
    );
}
