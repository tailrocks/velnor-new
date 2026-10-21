//! Publication job assembly: authority gate plus one build, attest and
//! publish chain per release-asset bundle.
//!
//! The authority job reads GitHub metadata only and checks out nothing.
//! Bundle jobs need it, so an unauthorized tag runs no bundle job. Only
//! the build job checks out code, and it holds read-only authority; the
//! attest and publish jobs work from the uploaded artifact alone.

use std::collections::BTreeMap;
use std::ffi::OsStr;

use velnor_actions_contract::config::{PublishBundle, VersionSource};
use velnor_actions_contract::{ImageConfig, JobTimeout, Step};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::publish_scripts as script;
use velnor_actions_workflow_renderer::publish_spec::{
    PublishJobSpec, PublishPermissions, PublishRole, PublishStep,
};
use velnor_actions_workflow_renderer::{MiseSetup, ambient_shell_step, mise_setup_step};

use crate::OrchestratorError;
use crate::publish_release_steps::{
    BRANCH_AUTHORITY_JOB, branch_authority_job, event_kind, release_jobs,
};
use crate::utf8::strings_of;
use bundle_steps::{attest_job, build_job, publish_job};
use image_steps::{crane_function, promote_job, publish_job as image_publish_job, publish_job_id};

#[path = "publish_bundle_steps.rs"]
mod bundle_steps;
#[path = "publish_maven_steps.rs"]
mod maven_steps;
pub(crate) use maven_steps::MavenPlan;
#[path = "publish_npm_steps.rs"]
mod npm_steps;
pub(crate) use npm_steps::NpmPlan;
#[path = "publish_image_steps.rs"]
pub(crate) mod image_steps;

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
    /// Resolved npm entries, sorted by id.
    pub(crate) npm: &'a [NpmPlan<'a>],
    /// Resolved Maven bundle entries, sorted by id.
    pub(crate) maven: &'a [MavenPlan<'a>],
    /// Manifest for the `git-tag` release job; `None` without that mode.
    pub(crate) git_tag_manifest: Option<&'a str>,
    /// Declared container images, sorted by id.
    pub(crate) images: &'a [ImageConfig],
    /// Images a release tag promotes, with their resolved version checks.
    pub(crate) promotions: &'a [PromotionPlan<'a>],
}

/// One promotable image plus its resolved version check.
pub(crate) struct PromotionPlan<'a> {
    /// Declared image (carries a supported `version_file`).
    pub(crate) image: &'a ImageConfig,
    /// Version check applied by the authority job.
    pub(crate) check: VersionCheck,
}

impl Inputs<'_> {
    /// Every artifact a release tag may name, bundles first, with its check.
    fn tag_artifacts(&self) -> Vec<(&str, &VersionCheck)> {
        self.bundles
            .iter()
            .map(|plan| (plan.bundle.id.as_str(), &plan.check))
            .chain(
                self.npm
                    .iter()
                    .map(|plan| (plan.entry.id.as_str(), &plan.check)),
            )
            .chain(
                self.maven
                    .iter()
                    .map(|plan| (plan.entry.id.as_str(), &plan.check)),
            )
            .chain(
                self.promotions
                    .iter()
                    .map(|plan| (plan.image.id.as_str(), &plan.check)),
            )
            .collect()
    }
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
    let branch_side = inputs.git_tag_manifest.is_some() || !inputs.images.is_empty();
    let tag_side = !inputs.bundles.is_empty()
        || !inputs.npm.is_empty()
        || !inputs.maven.is_empty()
        || !inputs.promotions.is_empty();
    let mixed = tag_side && branch_side;
    if tag_side {
        let mut authority = authority_job(inputs, &gh)?;
        if mixed {
            authority.condition = Some(event_kind("tag"));
        }
        jobs.insert(AUTHORITY_JOB.to_owned(), authority);
    }
    insert_tag_chains(inputs, &gh, &mut jobs)?;
    if branch_side {
        let mut authority = branch_authority_job(inputs, &gh)?;
        if mixed {
            authority.condition = Some(event_kind("branch"));
        }
        jobs.insert(BRANCH_AUTHORITY_JOB.to_owned(), authority);
    }
    if let Some(manifest) = inputs.git_tag_manifest {
        jobs.extend(release_jobs(inputs, manifest)?);
    }
    if !inputs.images.is_empty() {
        let crane = crane_function(inputs.catalog)?;
        for image in inputs.images {
            jobs.insert(
                publish_job_id(image),
                image_publish_job(inputs, image, &gh, &crane)?,
            );
        }
        for plan in inputs.promotions {
            jobs.insert(
                plan.image.promote_job_id(),
                promote_job(inputs, plan.image, &gh, &crane)?,
            );
        }
    }
    Ok(jobs)
}

/// The build, attest and publish chain of every tag-driven artifact kind.
fn insert_tag_chains(
    inputs: &Inputs<'_>,
    gh: &str,
    jobs: &mut BTreeMap<String, PublishJobSpec>,
) -> Result<(), OrchestratorError> {
    for plan in inputs.bundles {
        let id = plan.bundle.id.as_str();
        jobs.insert(
            format!("bundle-build-{id}"),
            build_job(inputs, plan.bundle)?,
        );
        jobs.insert(format!("bundle-attest-{id}"), attest_job(inputs, id)?);
        jobs.insert(format!("bundle-publish-{id}"), publish_job(inputs, id, gh)?);
    }
    for plan in inputs.npm {
        let id = plan.entry.id.as_str();
        jobs.insert(
            format!("npm-build-{id}"),
            npm_steps::build_job(inputs, plan)?,
        );
        jobs.insert(
            format!("npm-attest-{id}"),
            npm_steps::attest_job(inputs, plan.entry)?,
        );
        jobs.insert(
            format!("npm-publish-{id}"),
            npm_steps::publish_job(inputs, plan.entry)?,
        );
    }
    for plan in inputs.maven {
        let id = plan.entry.id.as_str();
        jobs.insert(
            format!("maven-build-{id}"),
            maven_steps::build_job(inputs, plan)?,
        );
        jobs.insert(
            format!("maven-attest-{id}"),
            maven_steps::attest_job(inputs, plan.entry)?,
        );
        jobs.insert(
            format!("maven-publish-{id}"),
            maven_steps::publish_job(inputs, plan.entry, gh)?,
        );
    }
    Ok(())
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
    let artifacts = inputs.tag_artifacts();
    let ids: Vec<&str> = artifacts.iter().map(|(id, _)| *id).collect();
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
    for (id, check) in artifacts {
        let (name, body, token) = match check {
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
        let step = script_step(&format!("{name} ({id})"), body, token)?;
        steps.push(plain(when_artifact(step, id)));
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
        services: Vec::new(),
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
    let publishing = matches!(
        role,
        PublishRole::ArtifactPublish | PublishRole::NpmPublish | PublishRole::TagRelease
    );
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
        services: Vec::new(),
    }
}
