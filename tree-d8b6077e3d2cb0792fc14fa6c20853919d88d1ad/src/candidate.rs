//! Candidate qualification and protected release jobs (Velnor only).
//!
//! The candidate job builds once, uploads with its manifest, downloads,
//! re-checks generated-tree freshness with the downloaded binary, then
//! qualifies the exact artifact without rebuilding it (bootstrap §4).
//! The release job publishes only qualified candidates on protected refs.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, JobTimeout, Step};

use crate::{
    closure::{FRESHNESS_OUTDIR, download_plan_step, freshness_step},
    render::{CANDIDATE_JOB_ID, CandidateSpec, PLAN_JOB_ID, RenderContext},
};
use velnor_actions_workflow_steps::{
    RenderError,
    artifact_paths::{CANDIDATE_OUTPUT_DIR_EXPR, CANDIDATE_STAGE_DIR_EXPR},
    steps,
};

/// Display name of the candidate-manifest verification step.
pub const VERIFY_MANIFEST_NAME: &str = "Verify candidate manifest";

/// Fail release qualification on non-release generator versions (GAP-E.1).
///
/// The release job calls this before publishing; pre-release and
/// malformed versions fail closed with `non_release_build`.
/// # Errors
pub fn check_release_build(version: &str, file: &str) -> Result<(), RenderError> {
    velnor_actions_contract_release::require_release_version(version, file)
        .map_err(RenderError::Contract)
}

/// Derived candidate artifact name for one target triple.
///
/// Cache-contract §4 fixes `velnor-candidate-<run-key>-<target-key>`; the
/// run key resolves from the workflow run at runtime while the target key
/// derives from the literal triple now (contract `target_key` grammar).
/// # Errors
pub fn candidate_artifact_name(target: &str) -> Result<String, RenderError> {
    let key = velnor_actions_contract::target_key(target).map_err(RenderError::Contract)?;
    Ok(format!("velnor-candidate-{}-{key}", steps::RUN_KEY_EXPR))
}

/// Fixed script verifying the downloaded manifest before any run.
///
/// Checks the manifest beside the downloaded binary: schema 1, 40-hex
/// commit equal to the checked-out `$GITHUB_SHA`, exact expected target,
/// nonempty toolchain, 64-hex sha256 equal to the binary's recomputed
/// digest. Pure POSIX `sh` parameter expansion plus `sha256sum`: no
/// command substitution (rejected by argv validation), no single quotes
/// (whole-script quoting, preseed precedent). The manifest writer emits
/// no trailing newline, so the `read` carries an `|| [ -n ... ]` guard.
#[must_use]
pub fn candidate_manifest_verify_script(target: &str) -> String {
    let dir = steps::CANDIDATE_STAGE_DIR;
    let file = steps::CANDIDATE_MANIFEST_FILE;
    format!(
        "line=; rest=; m=\"{dir}/{file}\" && b=\"{dir}/velnor-actions\" && test -f \"$m\" && test -x \"$b\" && read line rest < \"$m\" || [ -n \"$line\" ] && v=${{line#*\\\"schema\\\":}} && v=${{v%%,*}} && [ \"$v\" = 1 ] && c=${{line#*\\\"commit\\\":\\\"}} && c=${{c%%\\\"*}} && [ \"${{#c}}\" = 40 ] && [ \"$c\" = \"$GITHUB_SHA\" ] && t=${{line#*\\\"target\\\":\\\"}} && t=${{t%%\\\"*}} && [ \"$t\" = \"{target}\" ] && tc=${{line#*\\\"toolchain\\\":\\\"}} && tc=${{tc%%\\\"*}} && [ -n \"$tc\" ] && s=${{line#*\\\"sha256\\\":\\\"}} && s=${{s%%\\\"*}} && [ \"${{#s}}\" = 64 ] && sha256sum \"$b\" > \"{dir}/got.txt\" && read got rest < \"{dir}/got.txt\" && [ \"$got\" = \"$s\" ]"
    )
}

/// Manifest verification step; must precede every candidate execution.
///
/// Cache-contract §3: the report MUST verify the manifest before running
/// any candidate command. The expected target is the literal triple the
/// candidate job builds for (never the manifest's own claim). Scrubbed:
/// fixed `test`/`sha256sum` only, no auth needed.
/// # Errors
pub fn candidate_manifest_verify_step(target: &str) -> Result<Step, RenderError> {
    if !velnor_actions_contract_release::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "candidate_unsupported_target:{target}"
        )));
    }
    steps::shell_step(
        VERIFY_MANIFEST_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            candidate_manifest_verify_script(target),
        ],
        BTreeMap::new(),
    )
}

/// Candidate job: build once, attest the plan head, upload, verify, qualify.
///
/// Qualification runs against the downloaded artifact and never rebuilds
/// it (bootstrap contract §4 steps 4-5). Needs plan; feeds nothing.
/// Candidate mode downloads the plan, writes the head-bound attestation
/// beside the manifest (same artifact), verifies the downloaded
/// manifest, then performs the generated-files check with the downloaded
/// candidate binary, after the download and before qualification.
///
/// Credential posture: the build compiles PR source (build scripts run)
/// and qualification executes the PR-built binary, so both steps carry
/// the scrub overlay plus the unset wrapper. Neither needs auth: tool
/// and crate downloads are unauthenticated public fetches, so a cold
/// cache slows down (rate limits) instead of failing. A future private
/// registry would need an explicit scoped binding here, never ambient.
/// # Errors
pub(crate) fn candidate_job(ctx: &RenderContext, spec: &CandidateSpec) -> Result<Job, RenderError> {
    let target = velnor_actions_contract_release::ReleaseTarget::for_runner_label(&ctx.runs_on)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unsupported_target_for_runner:{}", ctx.runs_on))
        })?;
    let toolchain = toolchain_identity(&spec.build)?;
    let manifest = steps::candidate_manifest_script(target.triple(), &toolchain);
    let artifact = candidate_artifact_name(target.triple())?;
    let candidate_binary = format!("{}/velnor-actions", steps::CANDIDATE_STAGE_DIR);
    Ok(Job {
        display_name: "Candidate".to_owned(),
        runs_on: ctx.runs_on.clone(),
        check_runner: None,
        timeout_minutes: JobTimeout::CANDIDATE,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            steps::shell_step("Build candidate", spec.build.clone(), BTreeMap::new())?,
            download_plan_step()?,
            steps::shell_step(
                "Write candidate manifest",
                vec!["sh".to_owned(), "-c".to_owned(), manifest],
                BTreeMap::new(),
            )?,
            steps::shell_step(
                "Write candidate attestation",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    steps::candidate_attestation_script(),
                ],
                BTreeMap::new(),
            )?,
            steps::upload_artifact_step(&artifact, CANDIDATE_OUTPUT_DIR_EXPR)?,
            steps::download_artifact_step(&artifact, CANDIDATE_STAGE_DIR_EXPR)?,
            candidate_manifest_verify_step(target.triple())?,
            freshness_step(&candidate_binary, FRESHNESS_OUTDIR, &ctx.plan_consumer_env)?,
            steps::shell_step(
                "Qualify candidate",
                qualify_scrubbed_argv(&spec.qualify)?,
                BTreeMap::new(),
            )?,
        ],
    })
}

/// Qualification argv shape check: fixed `sh -c` script only.
///
/// The qualify vector executes the PR-built binary; anything else is
/// not a qualify vector and fails closed. Credential removal is
/// [`steps::shell_step`]'s job (argv-wide `env -u` prefix), not a
/// script prelude's, so this only gates the shape.
/// # Errors
fn qualify_scrubbed_argv(argv: &[String]) -> Result<Vec<String>, RenderError> {
    let [shell, flag, _script] = argv else {
        return Err(RenderError::BadCommand("qualify_without_unset".to_owned()));
    };
    if shell != "sh" || flag != "-c" {
        return Err(RenderError::BadCommand("qualify_without_unset".to_owned()));
    }
    Ok(argv.to_vec())
}

/// Protected release job: publish assets plus manifest, verify digests.
///
/// Runs only on protected refs (see `support::RELEASE_REF_CONDITION`);
/// publishes only the qualified candidate. The lock update lands as a
/// separate reviewed change (bootstrap contract §4 step 6), never here.
/// # Errors
pub(crate) fn release_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    let target = velnor_actions_contract_release::ReleaseTarget::for_runner_label(&ctx.runs_on)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unsupported_target_for_runner:{}", ctx.runs_on))
        })?;
    let artifact = candidate_artifact_name(target.triple())?;
    Ok(Job {
        display_name: super::support::RELEASE_DISPLAY_NAME.to_owned(),
        runs_on: ctx.runs_on.clone(),
        check_runner: None,
        timeout_minutes: JobTimeout::RELEASE,
        needs: vec![CANDIDATE_JOB_ID.to_owned()],
        condition: Some(super::support::RELEASE_REF_CONDITION.to_owned()),
        permissions: None,
        environment: None,
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            steps::download_artifact_step(&artifact, CANDIDATE_STAGE_DIR_EXPR)?,
            steps::ambient_shell_step(
                "Publish release assets",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "gh release upload \"$VELNOR_RELEASE_TAG\" \"$VELNOR_STAGE_DIR\"/velnor-actions --clobber".to_owned(),
                ],
                release_env(),
            )?,
            steps::ambient_shell_step(
                "Verify release digests",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "gh release download \"$VELNOR_RELEASE_TAG\" --dir \"$RUNNER_TEMP/velnor/verify\" --clobber && sha256sum -c \"$VELNOR_STAGE_DIR\"/candidate-manifest.json.sha256".to_owned(),
                ],
                release_env(),
            )?,
        ],
    })
}

/// Shared release-step env: release tag plus the staged candidate dir.
///
/// No credential keys: both `gh` steps run ambient (see
/// [`steps::ambient_shell_step`]) because publishing and verifying
/// release assets needs live forge auth. No repository code executes.
fn release_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "VELNOR_RELEASE_TAG".to_owned(),
            "${{ github.ref_name }}".to_owned(),
        ),
        (
            "VELNOR_STAGE_DIR".to_owned(),
            CANDIDATE_STAGE_DIR_EXPR.to_owned(),
        ),
    ])
}

/// Toolchain identity from the fixed build vector (`a@x+b@y`).
///
/// Joins the `tool@exact` specs between `exec` and `--`; fails when the
/// vector has no specs, so the manifest never carries a guessed toolchain.
pub(crate) fn toolchain_identity(build: &[String]) -> Result<String, RenderError> {
    let mut specs = Vec::new();
    let mut in_specs = false;
    for arg in build {
        if arg == "exec" {
            in_specs = true;
        } else if arg == "--" {
            break;
        } else if in_specs {
            specs.push(arg.clone());
        }
    }
    if specs.is_empty() || specs.iter().any(|spec| !spec.contains('@')) {
        return Err(RenderError::BadCommand(
            "candidate_build_without_toolchain".to_owned(),
        ));
    }
    Ok(specs.join("+"))
}
