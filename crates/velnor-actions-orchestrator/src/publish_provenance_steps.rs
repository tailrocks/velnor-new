// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub(crate) struct Subject<'a> {
    pub(crate) kind: &'a str,
    pub(crate) id: &'a str,
    pub(crate) dir: &'a str,
    pub(crate) file: String,
}

impl Subject<'_> {
    fn artifact(&self) -> String {
        format!("{}-provenance-{}", self.kind, self.id)
    }

    pub(crate) fn asset_names(&self) -> [String; 2] {
        [
            format!("{}{}", self.file, script::BUNDLE_SUFFIX),
            format!("{}{}", self.file, script::RECEIPT_SUFFIX),
        ]
    }
}

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

pub(crate) fn receipt_step(subject: &Subject<'_>) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(script_step(
        "Check provenance receipt",
        script::receipt_script(subject.dir, &subject.file),
        false,
    )?))
}

pub(crate) fn stage_step(subject: &Subject<'_>) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(script_step(
        "Stage provenance assets",
        script::stage_script(subject.dir, &subject.file),
        false,
    )?))
}

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
