//! Candidate qualification and protected release jobs (Velnor only).
//!
//! The candidate job builds once, uploads with its manifest, downloads,
//! re-checks generated-tree freshness with the downloaded binary, then
//! qualifies the exact artifact without rebuilding it (bootstrap §4).
//! The release job publishes only qualified candidates on protected refs.

use std::collections::BTreeMap;

use velnor_actions_contract::Job;

use crate::{
    RenderError,
    closure::{FRESHNESS_OUTDIR, freshness_step},
    render::{CANDIDATE_JOB_ID, CandidateSpec, PLAN_JOB_ID, RenderContext},
    steps,
};

/// Candidate job: build once, upload with manifest, download, qualify.
///
/// Qualification runs against the downloaded artifact and never rebuilds
/// it (bootstrap contract §4 steps 4-5). Needs plan; feeds nothing.
/// Candidate mode performs the generated-files check with the downloaded
/// candidate binary, after the download and before qualification.
/// # Errors
pub(crate) fn candidate_job(ctx: &RenderContext, spec: &CandidateSpec) -> Result<Job, RenderError> {
    let target =
        velnor_actions_contract::target_for_runner_label(&ctx.runs_on).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unsupported_target_for_runner:{}", ctx.runs_on))
        })?;
    let toolchain = toolchain_identity(&spec.build)?;
    let manifest = steps::candidate_manifest_script(target, &toolchain);
    let candidate_binary = format!("{}/velnor-actions", steps::CANDIDATE_STAGE_DIR);
    Ok(Job {
        display_name: "Velnor Candidate".to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            steps::shell_step("Build candidate", spec.build.clone(), BTreeMap::new())?,
            steps::shell_step(
                "Write candidate manifest",
                vec!["sh".to_owned(), "-c".to_owned(), manifest],
                BTreeMap::new(),
            )?,
            steps::upload_artifact_step(
                steps::CANDIDATE_ARTIFACT_NAME,
                steps::CANDIDATE_OUTPUT_DIR,
            )?,
            steps::download_artifact_step(
                steps::CANDIDATE_ARTIFACT_NAME,
                steps::CANDIDATE_STAGE_DIR,
            )?,
            freshness_step(&candidate_binary, FRESHNESS_OUTDIR)?,
            steps::shell_step("Qualify candidate", spec.qualify.clone(), BTreeMap::new())?,
        ],
    })
}

/// Protected release job: publish assets plus manifest, verify digests.
///
/// Runs only on protected refs (see `support::RELEASE_REF_CONDITION`);
/// publishes only the qualified candidate. The lock update lands as a
/// separate reviewed change (bootstrap contract §4 step 6), never here.
/// # Errors
pub(crate) fn release_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    Ok(Job {
        display_name: super::support::RELEASE_DISPLAY_NAME.to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: vec![CANDIDATE_JOB_ID.to_owned()],
        condition: Some(super::support::RELEASE_REF_CONDITION.to_owned()),
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            steps::download_artifact_step(
                steps::CANDIDATE_ARTIFACT_NAME,
                steps::CANDIDATE_STAGE_DIR,
            )?,
            steps::shell_step(
                "Publish release assets",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "gh release upload \"$VELNOR_RELEASE_TAG\" \"$VELNOR_STAGE_DIR\"/velnor-actions --clobber".to_owned(),
                ],
                release_env(),
            )?,
            steps::shell_step(
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
fn release_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "VELNOR_RELEASE_TAG".to_owned(),
            "${{ github.ref_name }}".to_owned(),
        ),
        (
            "VELNOR_STAGE_DIR".to_owned(),
            steps::CANDIDATE_STAGE_DIR.to_owned(),
        ),
    ])
}

/// Toolchain identity from the fixed build vector (`a@x+b@y`).
///
/// Joins the `tool@exact` specs between `exec` and `--`; fails when the
/// vector has no specs, so the manifest never carries a guessed toolchain.
fn toolchain_identity(build: &[String]) -> Result<String, RenderError> {
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
