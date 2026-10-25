use std::collections::BTreeMap;

use velnor_actions_actionlint::actions::{
    ATTEST_BUILD_PROVENANCE_USES, DOWNLOAD_ARTIFACT_ACTION_USES, UPLOAD_ARTIFACT_ACTION_USES,
};
use velnor_actions_contract::config::{PublishBundle, PublishProvenance};
use velnor_actions_workflow_renderer::publish_scripts as script;
use velnor_actions_workflow_renderer::publish_spec::{PublishJobSpec, PublishRole, PublishStep};
use velnor_actions_workflow_renderer::{action_step, checkout_step, mise_setup_step};

use super::provenance_steps::{self as provenance, Subject};
use super::{AUTHORITY_JOB, Inputs, bundle_job, plain, script_step};
use crate::OrchestratorError;
use crate::workflow::CHECKOUT_USES;

fn artifact_name(id: &str) -> String {
    format!("bundle-{id}")
}

fn download_step(id: &str) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(action_step(
        "Download bundle",
        DOWNLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name(id)),
            ("path".to_owned(), "${{ runner.temp }}/bundle".to_owned()),
        ]),
    )?))
}

pub(super) fn build_job(
    inputs: &Inputs<'_>,
    bundle: &PublishBundle,
) -> Result<PublishJobSpec, OrchestratorError> {
    let id = bundle.id.as_str();
    let upload = action_step(
        "Upload bundle",
        UPLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name(id)),
            ("path".to_owned(), "${{ runner.temp }}/bundle".to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?;
    let steps = vec![
        plain(checkout_step(CHECKOUT_USES)?),
        plain(script_step(
            "Pack bundle",
            script::pack_script(id, &bundle.paths),
            false,
        )?),
        plain(upload),
    ];
    let mut job = bundle_job(
        inputs,
        PublishRole::ArtifactBuild,
        format!("Bundle build ({id})"),
        AUTHORITY_JOB.to_owned(),
        steps,
    );
    job.condition = Some(format!("needs.{AUTHORITY_JOB}.outputs.artifact == '{id}'"));
    Ok(job)
}

fn subject(id: &str) -> Subject<'_> {
    Subject {
        kind: "bundle",
        id,
        dir: "$RUNNER_TEMP/bundle",
        file: format!("{id}-$ver.tar.gz"),
    }
}

fn checksum_step(id: &str) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(script_step(
        "Verify checksum",
        script::verify_checksum_script(id),
        false,
    )?))
}

pub(super) fn attest_job(
    inputs: &Inputs<'_>,
    id: &str,
    gh: &str,
) -> Result<PublishJobSpec, OrchestratorError> {
    if inputs.provenance == PublishProvenance::SigstoreBundle {
        return provenance::sign_job(
            inputs,
            gh,
            &subject(id),
            download_step(id)?,
            checksum_step(id)?,
            "Bundle",
        );
    }
    let attest = action_step(
        "Attest bundle provenance",
        ATTEST_BUILD_PROVENANCE_USES,
        BTreeMap::from([(
            "subject-path".to_owned(),
            "${{ runner.temp }}/bundle/*.tar.gz".to_owned(),
        )]),
    )?;
    let steps = vec![download_step(id)?, checksum_step(id)?, plain(attest)];
    Ok(bundle_job(
        inputs,
        PublishRole::ArtifactAttest,
        format!("Bundle attest ({id})"),
        format!("bundle-build-{id}"),
        steps,
    ))
}

pub(super) fn publish_job(
    inputs: &Inputs<'_>,
    id: &str,
    gh: &str,
) -> Result<PublishJobSpec, OrchestratorError> {
    let mut steps = vec![
        plain(mise_setup_step(inputs.mise)?),
        download_step(id)?,
        checksum_step(id)?,
    ];
    let upload = if inputs.provenance == PublishProvenance::SigstoreBundle {
        let subject = subject(id);
        steps.push(provenance::download_step(&subject)?);
        steps.push(provenance::verify_step(&subject, gh)?);
        steps.push(provenance::stage_step(&subject)?);
        let [bundle, receipt] = subject.asset_names();
        let names = [
            subject.file.clone(),
            format!("{}.sha256", subject.file),
            bundle,
            receipt,
        ];
        script::upload_files_script(gh, subject.dir, &names, "Release asset bundle")
    } else {
        script::upload_script(gh, id)
    };
    steps.push(plain(script_step("Upload release assets", upload, true)?));
    Ok(bundle_job(
        inputs,
        PublishRole::ArtifactPublish,
        format!("Bundle publish ({id})"),
        format!("bundle-attest-{id}"),
        steps,
    ))
}
