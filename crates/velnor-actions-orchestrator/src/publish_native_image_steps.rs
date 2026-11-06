// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_actionlint::{
    UPLOAD_ARTIFACT_ACTION_USES, actions::ATTEST_BUILD_PROVENANCE_USES,
};
use velnor_actions_contract::{ImageConfig, JobTimeout, Step, StepKind};
use velnor_actions_mise::PinnedTool;
use velnor_actions_workflow_renderer::{
    action_step, mise_setup_step,
    publish_spec::{PublishJobSpec, PublishRole, PublishStep},
};

use super::{AUTHORITY_JOB, Inputs, bundle_job, image_steps::IMAGE_ENVIRONMENT, plain};
use crate::{OrchestratorError, publish_release_steps::BRANCH_AUTHORITY_JOB};

pub(super) fn publish_job(
    inputs: &Inputs<'_>,
    image: &ImageConfig,
) -> Result<PublishJobSpec, OrchestratorError> {
    let publish = native_operation(
        "Publish authenticated native image",
        "native-image-publish-v1",
        inputs,
        image,
    );
    let attest = action_step(
        "Attest published image",
        ATTEST_BUILD_PROVENANCE_USES,
        BTreeMap::from([
            ("subject-name".to_owned(), image.repository.clone()),
            (
                "subject-digest".to_owned(),
                "${{ steps.image.outputs.digest }}".to_owned(),
            ),
            ("push-to-registry".to_owned(), "true".to_owned()),
            ("create-storage-record".to_owned(), "false".to_owned()),
        ]),
    )?;
    let mut upload = action_step(
        "Retain image publication journal",
        UPLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                format!(
                    "velnor-image-publication-{}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}",
                    image.id
                ),
            ),
            (
                "path".to_owned(),
                format!(
                    "${{{{ runner.temp }}}}/velnor-image-publication-{}.jsonl",
                    image.id
                ),
            ),
            ("if-no-files-found".to_owned(), "ignore".to_owned()),
            ("retention-days".to_owned(), "30".to_owned()),
        ]),
    )?;
    upload.condition = Some("always()".to_owned());
    let mut job = bundle_job(
        inputs,
        PublishRole::NativeImagePublish,
        format!("Publish native image ({})", image.id),
        BRANCH_AUTHORITY_JOB.to_owned(),
        vec![
            plain(mise_setup_step(inputs.mise)?),
            PublishStep {
                id: Some("image".to_owned()),
                step: publish,
            },
            plain(attest),
            plain(upload),
        ],
    );
    job.environment = Some(IMAGE_ENVIRONMENT.to_owned());
    job.timeout_minutes = JobTimeout::try_from(75)?;
    Ok(job)
}

pub(super) fn promote_job(
    inputs: &Inputs<'_>,
    image: &ImageConfig,
) -> Result<PublishJobSpec, OrchestratorError> {
    let promote = native_operation(
        "Promote authenticated native image digest",
        "native-image-promote-v1",
        inputs,
        image,
    );
    let mut job = bundle_job(
        inputs,
        PublishRole::NativeImagePromote,
        format!("Promote native image ({})", image.id),
        AUTHORITY_JOB.to_owned(),
        vec![plain(mise_setup_step(inputs.mise)?), plain(promote)],
    );
    job.condition = Some(format!(
        "needs.{AUTHORITY_JOB}.outputs.artifact == '{}'",
        image.id
    ));
    job.environment = Some(IMAGE_ENVIRONMENT.to_owned());
    job.timeout_minutes = JobTimeout::try_from(30)?;
    Ok(job)
}

fn native_operation(name: &str, operation: &str, inputs: &Inputs<'_>, image: &ImageConfig) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: operation.to_owned(),
            env: BTreeMap::from([
                ("IMAGE_PROFILE".to_owned(), image.id.clone()),
                ("IMAGE_REPOSITORY".to_owned(), image.repository.clone()),
                (
                    "GH_VERSION".to_owned(),
                    inputs.catalog.version(PinnedTool::Gh).to_owned(),
                ),
                (
                    "CRANE_VERSION".to_owned(),
                    inputs.catalog.version(PinnedTool::Crane).to_owned(),
                ),
            ]),
        },
    }
}
