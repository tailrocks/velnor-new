//! Publication job assembly: authority gate plus one build, attest and
//! publish chain per release-asset bundle.
//!
//! The authority job reads GitHub metadata only and checks out nothing.
//! Bundle jobs need it, so an unauthorized tag runs no bundle job. Only
//! the build job checks out code, and it holds read-only authority; the
//! attest and publish jobs work from the uploaded artifact alone.

use std::collections::BTreeMap;
use std::ffi::OsStr;

use velnor_actions_actionlint::actions::{
    ATTEST_BUILD_PROVENANCE_USES, DOWNLOAD_ARTIFACT_ACTION_USES, UPLOAD_ARTIFACT_ACTION_USES,
};
use velnor_actions_contract::config::{PublishBundle, VersionSource};
use velnor_actions_contract::{JobTimeout, Step};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::publish_scripts as script;
use velnor_actions_workflow_renderer::publish_spec::{
    PublishJobSpec, PublishPermissions, PublishRole, PublishStep,
};
use velnor_actions_workflow_renderer::{
    MiseSetup, action_step, ambient_shell_step, checkout_step, mise_setup_step,
};

use crate::OrchestratorError;
use crate::publish_emit::release::{event_kind, release_jobs};
use crate::utf8::strings_of;
use crate::workflow::CHECKOUT_USES;

/// Tag authority job id; every bundle job needs it.
pub(crate) const AUTHORITY_JOB: &str = "publish-authority";
/// Step id whose outputs carry the parsed tag.
const TAG_STEP: &str = "tag";
/// Token binding for GitHub API reads and release uploads.
const TOKEN_ENV: (&str, &str) = ("GH_TOKEN", "${{ secrets.GITHUB_TOKEN }}");

/// Where one bundle's version is checked.
pub(crate) enum VersionCheck {
    /// Inline literal baked into the workflow.
    Inline(String),
    /// Manifest read from the tagged commit.
    Manifest(VersionSource, String),
}

/// One bundle plus its resolved version check.
pub(crate) struct BundlePlan<'a> {
    /// Declared bundle.
    pub(crate) bundle: &'a PublishBundle,
    /// Version check applied by the authority job.
    pub(crate) check: VersionCheck,
}

/// Shared assembly inputs.
pub(crate) struct Inputs<'a> {
    /// Exact `owner/repo`.
    pub(crate) repository: &'a str,
    /// Default branch the tag must descend from.
    pub(crate) branch: &'a str,
    /// Literal runner label.
    pub(crate) label: &'a str,
    /// Protected environment for asset upload.
    pub(crate) environment: &'a str,
    /// Pinned Mise setup step inputs.
    pub(crate) mise: &'a MiseSetup,
    /// Pinned tool catalog.
    pub(crate) catalog: &'a ToolCatalog,
    /// Resolved bundles, sorted by id.
    pub(crate) bundles: &'a [BundlePlan<'a>],
    /// Manifest for the `git-tag` release job; `None` without that mode.
    pub(crate) git_tag_manifest: Option<&'a str>,
}

/// Assemble the authority job and every bundle chain.
///
/// # Errors
///
/// Returns render or contract errors for invalid steps or argv.
pub(crate) fn assemble_jobs(
    inputs: &Inputs<'_>,
) -> Result<BTreeMap<String, PublishJobSpec>, OrchestratorError> {
    let gh = gh_function(inputs.catalog)?;
    let mut jobs = BTreeMap::new();
    let mixed = !inputs.bundles.is_empty() && inputs.git_tag_manifest.is_some();
    if !inputs.bundles.is_empty() {
        let mut authority = authority_job(inputs, &gh)?;
        if mixed {
            authority.condition = Some(event_kind("tag"));
        }
        jobs.insert(AUTHORITY_JOB.to_owned(), authority);
    }
    for plan in inputs.bundles {
        let id = plan.bundle.id.as_str();
        jobs.insert(
            format!("bundle-build-{id}"),
            build_job(inputs, plan.bundle)?,
        );
        jobs.insert(format!("bundle-attest-{id}"), attest_job(inputs, id)?);
        jobs.insert(
            format!("bundle-publish-{id}"),
            publish_job(inputs, id, &gh)?,
        );
    }
    if let Some(manifest) = inputs.git_tag_manifest {
        jobs.extend(release_jobs(inputs, &gh, manifest, mixed)?);
    }
    Ok(jobs)
}

/// `gh` shell function body through the pinned Mise tool.
pub(crate) fn gh_function(catalog: &ToolCatalog) -> Result<String, OrchestratorError> {
    let exec =
        PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), Vec::new()).map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?;
    let prefix = strings_of(exec.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    Ok(script::gh_function(&prefix))
}

/// Plain step without an id.
pub(crate) fn plain(step: Step) -> PublishStep {
    PublishStep { id: None, step }
}

/// `sh -c` step running a fixed script with optional token binding.
pub(crate) fn script_step(
    name: &str,
    body: String,
    token: bool,
) -> Result<Step, OrchestratorError> {
    let env = if token {
        BTreeMap::from([(TOKEN_ENV.0.to_owned(), TOKEN_ENV.1.to_owned())])
    } else {
        BTreeMap::new()
    };
    Ok(ambient_shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), body],
        env,
    )?)
}

/// Only-for-this-bundle step condition on the parsed tag.
fn when_artifact(mut step: Step, id: &str) -> Step {
    step.condition = Some(format!("steps.{TAG_STEP}.outputs.artifact == '{id}'"));
    step
}

/// Authority: tag grammar, annotation, ancestry, `Required`, versions.
fn authority_job(inputs: &Inputs<'_>, gh: &str) -> Result<PublishJobSpec, OrchestratorError> {
    let ids: Vec<&str> = inputs
        .bundles
        .iter()
        .map(|p| p.bundle.id.as_str())
        .collect();
    let mut steps = vec![
        plain(mise_setup_step(inputs.mise)?),
        PublishStep {
            id: Some(TAG_STEP.to_owned()),
            step: script_step(
                "Parse release tag",
                script::parse_tag_script(inputs.repository, &ids),
                false,
            )?,
        },
        plain(script_step(
            "Verify annotated tag",
            script::annotated_tag_script(gh),
            true,
        )?),
        plain(script_step(
            "Verify default branch ancestry",
            script::ancestry_script(gh, inputs.branch),
            true,
        )?),
        plain(script_step(
            "Verify Required check",
            script::required_script(gh),
            true,
        )?),
    ];
    for plan in inputs.bundles {
        let (name, body, token) = match &plan.check {
            VersionCheck::Inline(version) => (
                "Verify configured version",
                script::inline_version_script(version),
                false,
            ),
            VersionCheck::Manifest(source, path) => (
                "Verify manifest version",
                script::manifest_script(gh, *source, path),
                true,
            ),
        };
        let step = script_step(&format!("{name} ({})", plan.bundle.id), body, token)?;
        steps.push(plain(when_artifact(step, &plan.bundle.id)));
    }
    let outputs = ["artifact", "version"]
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                format!("${{{{ steps.{TAG_STEP}.outputs.{name} }}}}"),
            )
        })
        .collect();
    Ok(PublishJobSpec {
        role: PublishRole::Authority,
        display_name: "Verify release authority".to_owned(),
        runs_on: inputs.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: Vec::new(),
        condition: None,
        environment: None,
        outputs,
        permissions: PublishPermissions::expected(PublishRole::Authority),
        steps,
    })
}

/// Common job shell for the three bundle roles.
pub(crate) fn bundle_job(
    inputs: &Inputs<'_>,
    role: PublishRole,
    name: String,
    needs: String,
    steps: Vec<PublishStep>,
) -> PublishJobSpec {
    let publishing = matches!(role, PublishRole::ArtifactPublish | PublishRole::TagRelease);
    PublishJobSpec {
        role,
        display_name: name,
        runs_on: inputs.label.to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: vec![needs],
        condition: None,
        environment: publishing.then(|| inputs.environment.to_owned()),
        outputs: BTreeMap::new(),
        permissions: PublishPermissions::expected(role),
        steps,
    }
}

/// Artifact name shared by upload and download.
fn artifact_name(id: &str) -> String {
    format!("bundle-{id}")
}

/// Download step restoring the bundle directory.
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

/// Build: checkout the tagged commit, pack deterministically, upload.
fn build_job(
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

/// Attest: verify the checksum, then sign provenance for the tarball.
fn attest_job(inputs: &Inputs<'_>, id: &str) -> Result<PublishJobSpec, OrchestratorError> {
    let attest = action_step(
        "Attest bundle provenance",
        ATTEST_BUILD_PROVENANCE_USES,
        BTreeMap::from([(
            "subject-path".to_owned(),
            "${{ runner.temp }}/bundle/*.tar.gz".to_owned(),
        )]),
    )?;
    let steps = vec![
        download_step(id)?,
        plain(script_step(
            "Verify checksum",
            script::verify_checksum_script(id),
            false,
        )?),
        plain(attest),
    ];
    Ok(bundle_job(
        inputs,
        PublishRole::ArtifactAttest,
        format!("Bundle attest ({id})"),
        format!("bundle-build-{id}"),
        steps,
    ))
}

/// Publish: re-verify, then upload immutable release assets.
fn publish_job(
    inputs: &Inputs<'_>,
    id: &str,
    gh: &str,
) -> Result<PublishJobSpec, OrchestratorError> {
    let steps = vec![
        plain(mise_setup_step(inputs.mise)?),
        download_step(id)?,
        plain(script_step(
            "Verify checksum",
            script::verify_checksum_script(id),
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
        format!("Bundle publish ({id})"),
        format!("bundle-attest-{id}"),
        steps,
    ))
}
