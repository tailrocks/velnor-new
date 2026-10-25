use std::collections::BTreeMap;
use std::ffi::OsStr;

use velnor_actions_contract::config::{PublishBundle, PublishProvenance, VersionSource};
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
#[path = "publish_provenance_steps.rs"]
mod provenance_steps;

pub(crate) const AUTHORITY_JOB: &str = "publish-authority";
const TAG_STEP: &str = "tag";
const TOKEN_ENV: (&str, &str) = ("GH_TOKEN", "${{ secrets.GITHUB_TOKEN }}");

pub(crate) enum VersionCheck {
    Inline(String),
    Manifest(VersionSource, String),
}

pub(crate) struct BundlePlan<'a> {
    pub(crate) bundle: &'a PublishBundle,
    pub(crate) check: VersionCheck,
}

pub(crate) struct Inputs<'a> {
    pub(crate) repository: &'a str,
    pub(crate) branch: &'a str,
    pub(crate) label: &'a str,
    pub(crate) environment: &'a str,
    pub(crate) provenance: PublishProvenance,
    pub(crate) mise: &'a MiseSetup,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) bundles: &'a [BundlePlan<'a>],
    pub(crate) npm: &'a [NpmPlan<'a>],
    pub(crate) maven: &'a [MavenPlan<'a>],
    pub(crate) git_tag_manifest: Option<&'a str>,
    pub(crate) images: &'a [ImageConfig],
    pub(crate) promotions: &'a [PromotionPlan<'a>],
}

pub(crate) struct PromotionPlan<'a> {
    pub(crate) image: &'a ImageConfig,
    pub(crate) check: VersionCheck,
}

impl Inputs<'_> {
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
        jobs.insert(format!("bundle-attest-{id}"), attest_job(inputs, id, gh)?);
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
            npm_steps::attest_job(inputs, plan.entry, gh)?,
        );
        if inputs.provenance == PublishProvenance::SigstoreBundle {
            jobs.insert(
                format!("npm-provenance-{id}"),
                npm_steps::provenance_job(inputs, plan.entry, gh)?,
            );
        }
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
            maven_steps::attest_job(inputs, plan.entry, gh)?,
        );
        jobs.insert(
            format!("maven-publish-{id}"),
            maven_steps::publish_job(inputs, plan.entry, gh)?,
        );
    }
    Ok(())
}

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

pub(crate) fn plain(step: Step) -> PublishStep {
    PublishStep { id: None, step }
}

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

fn when_artifact(mut step: Step, id: &str) -> Step {
    step.condition = Some(format!("steps.{TAG_STEP}.outputs.artifact == '{id}'"));
    step
}

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
