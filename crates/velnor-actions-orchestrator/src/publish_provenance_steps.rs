//! Shared jobs and steps for `[publish] provenance = "sigstore_bundle"`.
//!
//! The declared fallback keeps keyless Sigstore signing and changes only the
//! store. The sign job uses the existing `ArtifactAttest` role and checks out
//! no code while signing the verified artifact into a Sigstore bundle through
//! GitHub's Sigstore instance, verifies that bundle with
//! `gh attestation verify --bundle`, writes a provenance receipt and uploads
//! both as a run artifact. Every publishing job re-verifies them before it
//! publishes, then records them as immutable assets of the tag's release.
//! The default (`github_attestation`) never reaches this module, so an
//! undeclared repository keeps failing closed at the attestation store.

use std::collections::BTreeMap;

use velnor_actions_actionlint::actions::{
    DOWNLOAD_ARTIFACT_ACTION_USES, UPLOAD_ARTIFACT_ACTION_USES,
};
use velnor_actions_mise::{
    COSIGN_LINUX_AMD64_BYTES, COSIGN_LINUX_AMD64_SHA256, COSIGN_LINUX_ARM64_BYTES,
    COSIGN_LINUX_ARM64_SHA256, COSIGN_VERSION,
};
use velnor_actions_workflow_renderer::publish_scripts::provenance::{
    self as script, CosignPin, CosignPlatform,
};
use velnor_actions_workflow_renderer::publish_spec::{PublishJobSpec, PublishRole, PublishStep};
use velnor_actions_workflow_renderer::{action_step, mise_setup_step};

use super::{Inputs, bundle_job, plain, script_step};
use crate::OrchestratorError;

/// Resolve one of the six admitted Linux runner labels to its official asset.
fn cosign_for_runner(
    label: &str,
) -> Result<(CosignPin<'static>, CosignPlatform), OrchestratorError> {
    let (sha256, size, platform) = match label {
        "ubuntu-22.04" | "ubuntu-24.04" | "ubuntu-26.04" => (
            COSIGN_LINUX_AMD64_SHA256,
            COSIGN_LINUX_AMD64_BYTES,
            CosignPlatform::LinuxAmd64,
        ),
        "ubuntu-22.04-arm" | "ubuntu-24.04-arm" | "ubuntu-26.04-arm" => (
            COSIGN_LINUX_ARM64_SHA256,
            COSIGN_LINUX_ARM64_BYTES,
            CosignPlatform::LinuxArm64,
        ),
        _ => {
            return Err(OrchestratorError::Contract {
                problem: format!("unsupported_cosign_runner:{label}"),
            });
        }
    };
    Ok((
        CosignPin {
            version: COSIGN_VERSION,
            sha256,
            size,
        },
        platform,
    ))
}

/// Subject of one sign job: where the verified artifact sits after download.
pub(crate) struct Subject<'a> {
    /// Artifact kind, also the job and run-artifact prefix (`npm`, `maven`, `bundle`).
    pub(crate) kind: &'a str,
    /// Declared artifact id.
    pub(crate) id: &'a str,
    /// Fixed runner-temp directory holding the artifact.
    pub(crate) dir: &'a str,
    /// Validated file name inside `dir`; may use `$ver`.
    pub(crate) file: String,
}

impl Subject<'_> {
    /// Run-artifact name carrying the bundle and the receipt.
    fn artifact(&self) -> String {
        format!("{}-provenance-{}", self.kind, self.id)
    }

    /// Release asset names this subject adds in bundle mode.
    pub(crate) fn asset_names(&self) -> [String; 2] {
        [
            format!("{}{}", self.file, script::BUNDLE_SUFFIX),
            format!("{}{}", self.file, script::RECEIPT_SUFFIX),
        ]
    }
}

/// Download the bundle and receipt into the provenance directory.
pub(crate) fn download_step(subject: &Subject<'_>) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(action_step(
        "Download provenance bundle",
        DOWNLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            ("name".to_owned(), subject.artifact()),
            (
                "path".to_owned(),
                "${{ runner.temp }}/provenance".to_owned(),
            ),
        ]),
    )?))
}

/// Re-verify the bundle with `gh attestation verify --bundle` and check the receipt.
pub(crate) fn verify_step(
    subject: &Subject<'_>,
    gh: &str,
) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(script_step(
        "Verify provenance bundle",
        script::verify_script(gh, subject.dir, &subject.file),
        true,
    )?))
}

/// Check the receipt against the pushed bytes in a job without `gh`.
pub(crate) fn receipt_step(subject: &Subject<'_>) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(script_step(
        "Check provenance receipt",
        script::receipt_script(subject.dir, &subject.file),
        false,
    )?))
}

/// Copy the verified bundle and receipt beside the subject for the upload.
pub(crate) fn stage_step(subject: &Subject<'_>) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(script_step(
        "Stage provenance assets",
        script::stage_script(subject.dir, &subject.file),
        false,
    )?))
}

/// Sign job: download, verify checksum, sign and verify, upload bundle and receipt.
///
/// `download` and `checksum` are the kind's own steps, so the signed bytes
/// are exactly the ones the publish job re-verifies.
pub(crate) fn sign_job(
    inputs: &Inputs<'_>,
    gh: &str,
    subject: &Subject<'_>,
    download: PublishStep,
    checksum: PublishStep,
    display_kind: &str,
) -> Result<PublishJobSpec, OrchestratorError> {
    let (cosign, platform) = cosign_for_runner(inputs.label)?;
    let upload = action_step(
        "Upload provenance bundle",
        UPLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            ("name".to_owned(), subject.artifact()),
            (
                "path".to_owned(),
                "${{ runner.temp }}/provenance".to_owned(),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?;
    let steps = vec![
        plain(mise_setup_step(inputs.mise)?),
        download,
        checksum,
        plain(script_step(
            "Sign provenance bundle",
            script::sign_script_for_platform(gh, subject.dir, &subject.file, cosign, platform)
                .map_err(|problem| OrchestratorError::Contract {
                    problem: problem.to_owned(),
                })?,
            true,
        )?),
        plain(upload),
    ];
    Ok(bundle_job(
        inputs,
        PublishRole::ArtifactAttest,
        format!("{display_kind} sign ({})", subject.id),
        format!("{}-build-{}", subject.kind, subject.id),
        steps,
    ))
}

#[cfg(test)]
#[path = "publish_provenance_scripts_tests.rs"]
mod scripts_tests;

#[cfg(test)]
#[path = "publish_provenance_steps_tests.rs"]
mod tests;
