//! npm jobs of the publication workflow: build, attest, publish (HD-6, HD-8).
//!
//! The build job is the only one that checks out code and runs repository
//! scripts (the declared pack script, twice); it holds read-only authority.
//! Attest and publish work from the uploaded tarball alone. The publish job
//! holds `packages: write` in the protected environment, checks out nothing and
//! runs neither a package manager nor a lifecycle script.

use std::collections::BTreeMap;

use velnor_actions_actionlint::actions::{
    ATTEST_BUILD_PROVENANCE_USES, DOWNLOAD_ARTIFACT_ACTION_USES, UPLOAD_ARTIFACT_ACTION_USES,
};
use velnor_actions_contract::config::PublishNpm;
use velnor_actions_workflow_renderer::publish_scripts::npm as script;
use velnor_actions_workflow_renderer::publish_spec::{PublishJobSpec, PublishRole, PublishStep};
use velnor_actions_workflow_renderer::{action_step, checkout_step, mise_setup_step};

use super::{AUTHORITY_JOB, Inputs, bundle_job, plain, script_step};
use crate::OrchestratorError;
use crate::workflow::CHECKOUT_USES;

/// One npm entry plus the shell fragments built from the pinned Bun.
pub(crate) struct NpmPlan<'a> {
    /// Declared entry.
    pub(crate) entry: &'a PublishNpm,
    /// Version check applied by the authority job (the member's `package.json`).
    pub(crate) check: super::VersionCheck,
    /// `bun install --frozen-lockfile` for the workspace root.
    pub(crate) install: String,
    /// `bun run <pack_script>` at the workspace root.
    pub(crate) pack: String,
    /// Pinned-Bun preparation step.
    pub(crate) prepare: velnor_actions_contract::Step,
}

fn artifact_name(id: &str) -> String {
    format!("npm-{id}")
}

fn download_step(id: &str) -> Result<PublishStep, OrchestratorError> {
    Ok(plain(action_step(
        "Download tarball",
        DOWNLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name(id)),
            ("path".to_owned(), "${{ runner.temp }}/npm".to_owned()),
        ]),
    )?))
}

/// Build: checkout the tagged commit, install, pack twice, check, upload.
pub(super) fn build_job(
    inputs: &Inputs<'_>,
    plan: &NpmPlan<'_>,
) -> Result<PublishJobSpec, OrchestratorError> {
    let entry = plan.entry;
    let id = entry.id.as_str();
    let install = format!("set -eu; cd -P -- \"$GITHUB_WORKSPACE\"; {}", plan.install);
    let upload = action_step(
        "Upload tarball",
        UPLOAD_ARTIFACT_ACTION_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name(id)),
            ("path".to_owned(), "${{ runner.temp }}/npm".to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?;
    let steps = vec![
        plain(checkout_step(CHECKOUT_USES)?),
        plain(mise_setup_step(inputs.mise)?),
        plain(plan.prepare.clone()),
        plain(script_step("Install dependencies", install, false)?),
        plain(script_step(
            "Pack npm tarball",
            script::pack_script(&plan.pack, &entry.tarballs, &entry.member),
            false,
        )?),
        plain(upload),
    ];
    let mut job = bundle_job(
        inputs,
        PublishRole::ArtifactBuild,
        format!("npm build ({id})"),
        AUTHORITY_JOB.to_owned(),
        steps,
    );
    job.condition = Some(format!("needs.{AUTHORITY_JOB}.outputs.artifact == '{id}'"));
    Ok(job)
}

/// Attest: verify the checksum, then sign provenance for the tarball.
pub(super) fn attest_job(
    inputs: &Inputs<'_>,
    entry: &PublishNpm,
) -> Result<PublishJobSpec, OrchestratorError> {
    let id = entry.id.as_str();
    let attest = action_step(
        "Attest tarball provenance",
        ATTEST_BUILD_PROVENANCE_USES,
        BTreeMap::from([(
            "subject-path".to_owned(),
            "${{ runner.temp }}/npm/*.tgz".to_owned(),
        )]),
    )?;
    let steps = vec![
        download_step(id)?,
        plain(script_step(
            "Verify checksum",
            script::verify_script(&entry.member),
            false,
        )?),
        plain(attest),
    ];
    Ok(bundle_job(
        inputs,
        PublishRole::ArtifactAttest,
        format!("npm attest ({id})"),
        format!("npm-build-{id}"),
        steps,
    ))
}

/// Publish: re-verify, then `PUT` the package document with the job token.
pub(super) fn publish_job(
    inputs: &Inputs<'_>,
    entry: &PublishNpm,
) -> Result<PublishJobSpec, OrchestratorError> {
    let id = entry.id.as_str();
    let steps = vec![
        download_step(id)?,
        plain(script_step(
            "Verify checksum",
            script::verify_script(&entry.member),
            false,
        )?),
        plain(script_step(
            "Publish to GitHub Packages",
            script::publish_script(&entry.member),
            true,
        )?),
    ];
    Ok(bundle_job(
        inputs,
        PublishRole::NpmPublish,
        format!("npm publish ({id})"),
        format!("npm-attest-{id}"),
        steps,
    ))
}
