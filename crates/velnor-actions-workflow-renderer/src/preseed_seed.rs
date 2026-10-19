//! Pre-seed helper consumers of the Cache seed workflow.
//!
//! In bootstrap repositories the helper is built once by the triggering CI
//! run's Plan job. The seed downloads that exact artifact from the
//! triggering run (`run-id` plus a read token) and verifies its manifest
//! against the run's head commit (the commit the seed checked out), not
//! the `workflow_run` event's `GITHUB_SHA` (the default branch tip).

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;
use crate::preseed::{
    PRESEED_ARTIFACT_NAME, PRESEED_DOWNLOAD_NAME, PRESEED_VERIFY_MANIFEST_NAME,
    manifest_verify_script,
};
use crate::steps;

/// Env carrying the triggering run's head commit into the verify step.
pub const SEED_HEAD_SHA_ENV: &str = "VELNOR_SEED_HEAD_SHA";

/// Download the helper the triggering CI run's Plan job uploaded.
/// # Errors
pub fn seed_preseed_download_step() -> Result<Step, RenderError> {
    let mut step = steps::action_step(
        PRESEED_DOWNLOAD_NAME,
        steps::DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("github-token".to_owned(), "${{ github.token }}".to_owned()),
            ("name".to_owned(), PRESEED_ARTIFACT_NAME.to_owned()),
            (
                "path".to_owned(),
                crate::artifact_paths::PRESEED_STAGE_DIR_EXPR.to_owned(),
            ),
            (
                "run-id".to_owned(),
                "${{ github.event.workflow_run.id }}".to_owned(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::PreseedDownload);
    Ok(step)
}

/// Verify the downloaded manifest against the triggering run's head commit.
/// # Errors
pub fn seed_preseed_manifest_verify_step(target: &str) -> Result<Step, RenderError> {
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "preseed_unsupported_target:{target}"
        )));
    }
    let mut step = steps::shell_step(
        PRESEED_VERIFY_MANIFEST_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            manifest_verify_script(target, SEED_HEAD_SHA_ENV),
        ],
        BTreeMap::from([(
            SEED_HEAD_SHA_ENV.to_owned(),
            "${{ github.event.workflow_run.head_sha }}".to_owned(),
        )]),
    )?;
    step.role = Some(StepRole::PreseedVerifyManifest);
    Ok(step)
}

#[cfg(test)]
mod tests {
    use velnor_actions_contract::StepKind;

    #[test]
    fn the_seed_binds_the_helper_to_the_triggering_run_and_head() {
        let download = super::seed_preseed_download_step().expect("download");
        let StepKind::Action { with, .. } = &download.kind else {
            panic!("download is an action");
        };
        assert_eq!(with["run-id"], "${{ github.event.workflow_run.id }}");
        assert_eq!(with["github-token"], "${{ github.token }}");
        let verify =
            super::seed_preseed_manifest_verify_step("x86_64-unknown-linux-gnu").expect("verify");
        let StepKind::Shell { run, env } = &verify.kind else {
            panic!("verify is shell");
        };
        assert!(run[2].contains("[ \"$c\" = \"$VELNOR_SEED_HEAD_SHA\" ]"));
        assert!(!run[2].contains("GITHUB_SHA"));
        assert_eq!(
            env[super::SEED_HEAD_SHA_ENV],
            "${{ github.event.workflow_run.head_sha }}"
        );
    }
}
