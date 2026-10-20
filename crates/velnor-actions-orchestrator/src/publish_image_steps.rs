//! Image publication jobs of `publish.yml`.
//!
//! The unprivileged build jobs of `ci.yml` leave one OCI layout per platform
//! as an artifact. On a push to the default branch, after the branch
//! authority job saw a green `Required` check, one `image-publish-<id>` job
//! per image downloads those layouts from the push's CI run, verifies them,
//! assembles one multi-platform index, pushes it to the registry and attests
//! it. The job checks out nothing and executes no repository code: every
//! script is a generator constant filled with contract-validated values.

use std::collections::BTreeMap;
use std::ffi::OsStr;

use velnor_actions_actionlint::actions::{ATTEST_BUILD_PROVENANCE_USES, ATTEST_USES};
use velnor_actions_contract::{ImageConfig, JobTimeout};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::image_publish_scripts as script;
use velnor_actions_workflow_renderer::publish_spec::{PublishJobSpec, PublishRole, PublishStep};
use velnor_actions_workflow_renderer::{action_step, mise_setup_step};

use super::{Inputs, bundle_job, plain, script_step};
use crate::OrchestratorError;
use crate::publish_release_steps::BRANCH_AUTHORITY_JOB;
use crate::utf8::strings_of;

/// Protected environment guarding registry writes.
pub(crate) const IMAGE_ENVIRONMENT: &str = "images";

/// Publish job id for one image.
pub(crate) fn publish_job_id(image: &ImageConfig) -> String {
    image.publish_job_id()
}

/// `crane` shell function body through the pinned Mise tool.
pub(crate) fn crane_function(catalog: &ToolCatalog) -> Result<String, OrchestratorError> {
    let exec = PinnedToolExec::new(vec![PinnedTool::Crane], OsStr::new("crane"), Vec::new())
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let prefix = strings_of(exec.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    Ok(script::crane_function(&prefix))
}

/// The publish job of one image.
///
/// `branch_tag` is the moving tag naming the default branch, present only
/// when the branch name is a valid image tag.
pub(crate) fn publish_job(
    inputs: &Inputs<'_>,
    image: &ImageConfig,
    gh: &str,
    crane: &str,
) -> Result<PublishJobSpec, OrchestratorError> {
    let id = image.id.as_str();
    let owner = image
        .repository
        .split('/')
        .nth(1)
        .ok_or_else(|| OrchestratorError::Contract {
            problem: format!("images.{id}:image_repository_owner"),
        })?;
    let branch_tag = is_image_tag(inputs.branch).then_some(inputs.branch);
    let step = |name: &str, body: String, token: bool| script_step(name, body, token);
    let mut steps = vec![
        plain(mise_setup_step(inputs.mise)?),
        plain(step(
            "Verify image registry owner",
            script::owner_script(owner),
            false,
        )?),
        plain(step(
            "Download image layouts",
            script::download_script(gh, id, inputs.branch, &image.platforms),
            true,
        )?),
        PublishStep {
            id: Some(script::ASSEMBLE_STEP_ID.to_owned()),
            step: step(
                "Assemble multi-platform index",
                script::assemble_script(id, &image.platforms),
                false,
            )?,
        },
        plain(step(
            "Log in to the registry",
            script::login_script(crane),
            true,
        )?),
        plain(step(
            "Push image",
            script::push_script(crane, id, &image.repository, branch_tag),
            false,
        )?),
        plain(provenance_step(image)?),
    ];
    for platform in &image.platforms {
        steps.push(plain(sbom_step(image, platform.job_suffix())?));
    }
    steps.push(plain(step(
        "Verify attestations",
        script::verify_script(gh, id, &image.repository, &image.platforms),
        true,
    )?));
    let mut job = bundle_job(
        inputs,
        PublishRole::ImagePublish,
        format!("Image publish ({id})"),
        BRANCH_AUTHORITY_JOB.to_owned(),
        steps,
    );
    job.environment = Some(IMAGE_ENVIRONMENT.to_owned());
    job.timeout_minutes = JobTimeout::try_from(60)?;
    Ok(job)
}

/// Build provenance for the pushed index digest, stored with the image.
fn provenance_step(
    image: &ImageConfig,
) -> Result<velnor_actions_contract::Step, OrchestratorError> {
    Ok(action_step(
        "Attest image provenance",
        ATTEST_BUILD_PROVENANCE_USES,
        BTreeMap::from([
            ("subject-name".to_owned(), image.repository.clone()),
            ("subject-digest".to_owned(), output("digest")),
            ("push-to-registry".to_owned(), "true".to_owned()),
            ("create-storage-record".to_owned(), "false".to_owned()),
        ]),
    )?)
}

/// SPDX SBOM attestation for one platform image manifest.
fn sbom_step(
    image: &ImageConfig,
    arch: &str,
) -> Result<velnor_actions_contract::Step, OrchestratorError> {
    Ok(action_step(
        &format!("Attest image SBOM ({arch})"),
        ATTEST_USES,
        BTreeMap::from([
            ("subject-name".to_owned(), image.repository.clone()),
            (
                "subject-digest".to_owned(),
                output(&format!("digest_{arch}")),
            ),
            (
                "sbom-path".to_owned(),
                format!(
                    "${{{{ runner.temp }}}}/velnor/image/{}/sbom/{arch}.spdx.json",
                    image.id
                ),
            ),
            ("push-to-registry".to_owned(), "true".to_owned()),
            ("create-storage-record".to_owned(), "false".to_owned()),
        ]),
    )?)
}

/// Expression reading one output of the assemble step.
fn output(name: &str) -> String {
    format!(
        "${{{{ steps.{}.outputs.{name} }}}}",
        script::ASSEMBLE_STEP_ID
    )
}

/// True for a name usable as a registry tag without escaping.
fn is_image_tag(name: &str) -> bool {
    name.len() <= 128
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

#[cfg(test)]
#[path = "publish_image_scripts_tests.rs"]
mod scripts_tests;
