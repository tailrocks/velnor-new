// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::production::operation_env;
use super::{
    ConsumerReleaseSpec, RenderError, TapInvocationContext, Yaml, acquire, operation, permissions,
    setup, steps,
};
use crate::{
    commands::join_argv_for_run,
    schema2::features::{base, finish, run_step},
};
const SOURCE_ADMISSION_PATH: &str = "${{ runner.temp }}/velnor/preview-source-admission.json";

pub(super) fn job(spec: &ConsumerReleaseSpec) -> Result<(String, Yaml), RenderError> {
    let mut fields = base(
        "Select authenticated main source for preview",
        Yaml::str("ubuntu-26.04"),
        spec.publication_timeout_minutes,
    );
    fields.push((
        "if".to_owned(),
        Yaml::str(
            "${{ github.event_name == 'workflow_dispatch' || (github.event_name == 'workflow_run' && github.event.workflow_run.conclusion == 'success') }}",
        ),
    ));
    fields.push((
        "permissions".to_owned(),
        permissions(&[("actions", "read"), ("contents", "read")]),
    ));
    let mut job_steps = vec![
        setup(&spec.linux_setup),
        run_step(
            "Install pinned GitHub CLI",
            &join_argv_for_run(&spec.install_gh_argv)?,
        ),
        acquire(&spec.linux_acquire, "preview-eligibility", "ubuntu-26.04")?,
        source_select_step(spec)?,
    ];
    job_steps.push(steps::checkout_with_lfs(
        spec,
        "${{ steps.source-select.outputs.source_sha }}",
        "${{ steps.source-select.outputs.checkout_lfs }}",
    ));
    job_steps.push(finalize_step(spec)?);
    job_steps.push(source_admission_upload());
    fields.push(("outputs".to_owned(), job_outputs()));
    Ok(finish("preview-eligibility", fields, job_steps))
}

fn job_outputs() -> Yaml {
    let mut outputs = steps::outputs(&[
        "commit_count",
        "release_policy",
        "payload_matrix",
        "producer_runner",
        "producer_timeout_minutes",
        "source_admission_sha256",
    ]);
    if let Yaml::Map(outputs) = &mut outputs {
        for key in [
            "source_sha",
            "workflow_authority_sha",
            "ci_run_id",
            "ci_attempt",
            "checkout_lfs",
        ] {
            outputs.push((
                key.to_owned(),
                Yaml::str(format!("${{{{ steps.source-select.outputs.{key} }}}}")),
            ));
        }
        outputs.push((
            "source_admission_artifact_id".to_owned(),
            Yaml::str("${{ steps.source-admission-upload.outputs.artifact-id }}"),
        ));
        outputs.push((
            "source_admission_artifact_digest".to_owned(),
            Yaml::str("${{ steps.source-admission-upload.outputs.artifact-digest }}"),
        ));
    }
    outputs
}

fn source_select_step(spec: &ConsumerReleaseSpec) -> Result<Yaml, RenderError> {
    let mut step = operation(
        spec,
        "select-preview-source-v1",
        true,
        TapInvocationContext::Preview,
    )?;
    set_id(&mut step, "source-select");
    Ok(step)
}

fn finalize_step(spec: &ConsumerReleaseSpec) -> Result<Yaml, RenderError> {
    let mut step = operation(
        spec,
        "finalize-preview-source-v1",
        true,
        TapInvocationContext::Preview,
    )?;
    operation_env(
        &mut step,
        &[
            (
                "VELNOR_RELEASE_SOURCE_SHA",
                "${{ steps.source-select.outputs.source_sha }}",
            ),
            (
                "VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA",
                "${{ steps.source-select.outputs.workflow_authority_sha }}",
            ),
            (
                "VELNOR_RELEASE_CI_RUN_ID",
                "${{ steps.source-select.outputs.ci_run_id }}",
            ),
            (
                "VELNOR_RELEASE_CI_RUN_ATTEMPT",
                "${{ steps.source-select.outputs.ci_attempt }}",
            ),
        ],
    )?;
    set_id(&mut step, "source");
    Ok(step)
}

fn source_admission_upload() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Retain authenticated preview source admission"),
        ),
        ("id".to_owned(), Yaml::str("source-admission-upload")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "name".to_owned(),
                    Yaml::str(
                        "velnor-preview-source-r${{ github.run_id }}-a${{ github.run_attempt }}",
                    ),
                ),
                ("path".to_owned(), Yaml::str(SOURCE_ADMISSION_PATH)),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                (
                    "retention-days".to_owned(),
                    Yaml::Int(i64::from(crate::steps::ARTIFACT_RETENTION_DAYS)),
                ),
                ("overwrite".to_owned(), Yaml::Bool(false)),
                ("include-hidden-files".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

fn set_id(step: &mut Yaml, id: &str) {
    if let Yaml::Map(fields) = step
        && let Some((_, value)) = fields.iter_mut().find(|(key, _)| key == "id")
    {
        *value = Yaml::str(id);
    }
}
