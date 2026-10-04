//! Candidate execution on an isolated, read-only qualification runner.

use crate::RenderError;
use crate::yaml::Yaml;

use super::super::features::{base, finish};
use super::assets::ProductAsset;
use super::workflow_steps::{self, with_permissions};
use super::{assets, jobs};

/// Qualify a build artifact in a job that has no write or attestation permissions.
pub(super) fn job(
    id: &str,
    name: &str,
    action: &str,
    runs_on: Yaml,
    build_job: &str,
    product: ProductAsset,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let mut action_steps = Vec::new();
    action_steps.extend(assets::download_build_steps(
        product,
        "Download built asset archive",
    ));
    action_steps.extend([
        workflow_steps::bash_step(
            "Verify candidate provenance record",
            &assets::verify_provenance_script(product),
        ),
        workflow_steps::bash_step(
            "Verify downloaded checksum sidecar",
            &format!(
                "set -eu\ncd {}\n{} {}",
                product.directory, product.checksum_command, product.sidecar
            ),
        ),
        workflow_steps::bash_step(
            "Qualify downloaded candidate",
            &assets::qualification_script(product.binary, product.directory),
        ),
    ]);
    let call = jobs::local_action_with_input(
        action,
        name,
        action_steps,
        "artifact_id",
        "Artifact ID from this target's build job",
        &format!("${{{{ needs.{build_job}.outputs.artifact_id }}}}"),
        actions,
    )?;
    Ok(finish(
        id,
        with_permissions(
            workflow_steps::with_needs(base(name, runs_on, 120), &[build_job]),
            workflow_steps::qualification_permissions(),
        ),
        vec![
            workflow_steps::bash_step(
                "Fetch exact public source without an action post hook",
                super::source::QUALIFICATION_SOURCE_PREPARE,
            ),
            call,
        ],
    ))
}
