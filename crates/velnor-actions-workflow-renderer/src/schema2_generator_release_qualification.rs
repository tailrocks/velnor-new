//! Candidate execution on an isolated, read-only qualification runner.

use crate::RenderError;
use crate::yaml::Yaml;

use super::super::features::{base, finish};
use super::assets::ProductAsset;
use super::workflow_steps::{self, with_permissions};
use super::{GeneratorReleasePins, assets, jobs, manifest};

/// Qualify a build artifact in a job that has no write or attestation permissions.
pub(super) fn job(
    id: &str,
    name: &str,
    action: &str,
    runs_on: Yaml,
    build_job: &str,
    product: ProductAsset,
    pins: &GeneratorReleasePins,
    source_action: &Yaml,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let mut action_steps = Vec::new();
    action_steps.extend(assets::download_build_steps(
        product,
        "Download built asset archive",
    ));
    action_steps.push(workflow_steps::download_step_by_id(
        "Download canonical same-run candidate manifest",
        "${{ inputs.manifest_artifact_id }}",
        manifest::DIR,
    ));
    action_steps.extend([
        workflow_steps::bash_step(
            "Verify candidate provenance record",
            &assets::verify_provenance_script(product, pins),
        ),
        workflow_steps::bash_step(
            "Verify downloaded checksum sidecar",
            &format!(
                "set -eu\ncd {}\n{} {}",
                product.directory, product.checksum_command, product.sidecar
            ),
        ),
        workflow_steps::bash_step_with_env(
            "Qualify downloaded candidate",
            &assets::qualification_script(product.binary, product.directory),
            vec![(
                "VELNOR_RELEASE_MANIFEST_SHA256",
                "${{ inputs.manifest_sha256 }}",
            )],
        ),
    ]);
    let call = jobs::local_action_with_inputs(
        action,
        name,
        action_steps,
        vec![
            (
                "artifact_id",
                "Artifact ID from this target's build job",
                &format!("${{{{ needs.{build_job}.outputs.artifact_id }}}}"),
            ),
            (
                "manifest_artifact_id",
                "Artifact ID of the canonical candidate manifest",
                "${{ needs.candidate-manifest.outputs.artifact_id }}",
            ),
            (
                "manifest_sha256",
                "SHA-256 of the canonical candidate manifest bytes",
                "${{ needs.candidate-manifest.outputs.manifest_sha256 }}",
            ),
        ],
        actions,
    )?;
    Ok(finish(
        id,
        with_permissions(
            workflow_steps::with_needs(
                base(name, runs_on, 120),
                &[build_job, "candidate-manifest"],
            ),
            workflow_steps::qualification_permissions(),
        ),
        vec![source_action.clone(), call],
    ))
}
