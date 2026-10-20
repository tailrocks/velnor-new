//! Maven bundle jobs of the publication workflow: build, attest, publish (HD-6, HD-8).
//!
//! The build job is the only one that checks out code and runs repository
//! build logic (the Gradle root's `:mavenBundle`); it holds read-only authority
//! and the short-lived private-source token only inside the mint, fetch and
//! revoke steps that surround it. Attest and publish work from the uploaded
//! zip alone: they check out nothing and execute no repository code.

use std::collections::BTreeMap;

use velnor_actions_actionlint::actions::{
    ATTEST_BUILD_PROVENANCE_USES, DOWNLOAD_ARTIFACT_ACTION_USES, UPLOAD_ARTIFACT_ACTION_USES,
};
use velnor_actions_contract::JobTimeout;
use velnor_actions_contract::config::PublishMaven;
use velnor_actions_workflow_renderer::publish_scripts::maven as script;
use velnor_actions_workflow_renderer::publish_spec::{PublishJobSpec, PublishRole, PublishStep};
use velnor_actions_workflow_renderer::{action_step, checkout_step, mise_setup_step};

use super::{AUTHORITY_JOB, Inputs, VersionCheck, bundle_job, plain, script_step};
use crate::OrchestratorError;
use crate::workflow::CHECKOUT_USES;

/// One Maven entry plus the steps built from the Gradle stack.
pub(crate) struct MavenPlan<'a> {
    /// Declared entry.
    pub(crate) entry: &'a PublishMaven,
    /// Version check applied by the authority job (the root's `gradle.properties`).
    pub(crate) check: VersionCheck,
    /// Staged-helper acquisition plus the private-source block, in order.
    pub(crate) prefix: Vec<PublishStep>,
    /// The `gradle-run-v1` step running `:mavenBundle`.
    pub(crate) gradle: velnor_actions_contract::Step,
    /// Build bound: the Gradle stack's own timeout, never the five-minute release default.
    pub(crate) timeout: JobTimeout,
}

/// Artifact name shared by upload and download.
fn artifact_name(id: &str) -> String {
    format!("maven-{id}")
}

fn download_step(id: &str) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(action_step(
        "Download Maven bundle",
        DOWNLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name(id)),
            ("path".to_owned(), "${{ runner.temp }}/bundle".to_owned()),
        ]),
    )?))
}

/// Build: checkout the tagged commit, run `:mavenBundle`, verify, upload.
pub(super) fn build_job(
    inputs: &Inputs<'_>,
    plan: &MavenPlan<'_>,
) -> Result<PublishJobSpec, OrchestratorError> {
    let entry = plan.entry;
    let id = entry.id.as_str();
    let upload = action_step(
        "Upload Maven bundle",
        UPLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name(id)),
            ("path".to_owned(), "${{ runner.temp }}/bundle".to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?;
    let mut steps = vec![plain(checkout_step(CHECKOUT_USES)?)];
    steps.extend(plan.prefix.iter().cloned());
    steps.push(plain(mise_setup_step(inputs.mise)?));
    steps.push(plain(plan.gradle.clone()));
    steps.push(plain(script_step(
        "Collect Maven bundle",
        script::collect_script(id, &entry.output_dir()),
        false,
    )?));
    steps.push(plain(upload));
    let mut job = bundle_job(
        inputs,
        PublishRole::ArtifactBuild,
        format!("Maven build ({id})"),
        AUTHORITY_JOB.to_owned(),
        steps,
    );
    job.condition = Some(format!("needs.{AUTHORITY_JOB}.outputs.artifact == '{id}'"));
    job.timeout_minutes = plan.timeout;
    Ok(job)
}

/// Attest: verify the checksum, then sign provenance for the zip.
pub(super) fn attest_job(
    inputs: &Inputs<'_>,
    entry: &PublishMaven,
) -> Result<PublishJobSpec, OrchestratorError> {
    let id = entry.id.as_str();
    let attest = action_step(
        "Attest Maven bundle provenance",
        ATTEST_BUILD_PROVENANCE_USES,
        BTreeMap::from([(
            "subject-path".to_owned(),
            "${{ runner.temp }}/bundle/*.zip".to_owned(),
        )]),
    )?;
    let steps = vec![
        download_step(id)?,
        plain(script_step(
            "Verify checksum",
            script::verify_script(id),
            false,
        )?),
        plain(attest),
    ];
    Ok(bundle_job(
        inputs,
        PublishRole::ArtifactAttest,
        format!("Maven attest ({id})"),
        format!("maven-build-{id}"),
        steps,
    ))
}

/// Publish: re-verify, then upload immutable release assets.
pub(super) fn publish_job(
    inputs: &Inputs<'_>,
    entry: &PublishMaven,
    gh: &str,
) -> Result<PublishJobSpec, OrchestratorError> {
    let id = entry.id.as_str();
    let steps = vec![
        plain(mise_setup_step(inputs.mise)?),
        download_step(id)?,
        plain(script_step(
            "Verify checksum",
            script::verify_script(id),
            false,
        )?),
        plain(script_step(
            "Upload release assets",
            script::upload_script(gh, id),
            true,
        )?),
    ];
    Ok(bundle_job(
        inputs,
        PublishRole::ArtifactPublish,
        format!("Maven publish ({id})"),
        format!("maven-attest-{id}"),
        steps,
    ))
}
