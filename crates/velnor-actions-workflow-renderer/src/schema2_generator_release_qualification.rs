//! Candidate execution on an isolated, read-only qualification runner.

use crate::RenderError;
use crate::yaml::Yaml;

use super::super::features::{base, finish};
use super::assets::ProductAsset;
use super::workflow_steps::{self, with_permissions};
use super::{GeneratorReleasePins, assets, jobs, manifest};

pub(super) struct QualificationJob<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub action: &'a str,
    pub runs_on: Yaml,
    pub build_job: &'a str,
    pub product: ProductAsset,
    pub source_step: &'a Yaml,
}

/// Qualify a build artifact in a job that has no write or attestation permissions.
pub(super) fn job(
    job: QualificationJob<'_>,
    pins: &GeneratorReleasePins,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let mut action_steps = vec![workflow_steps::mise_step(
        pins.setup_for(job.product.target),
    )?];
    action_steps.extend(assets::download_build_steps(
        job.product,
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
            &assets::verify_provenance_script(job.product, pins),
        ),
        workflow_steps::bash_step(
            "Verify downloaded checksum sidecar",
            &format!(
                "set -eu\ncd {}\n{} {}",
                job.product.directory, job.product.checksum_command, job.product.sidecar
            ),
        ),
        workflow_steps::bash_step_with_env(
            "Qualify downloaded candidate",
            &assets::qualification_script(job.product.binary, job.product.directory),
            vec![(
                "VELNOR_RELEASE_MANIFEST_SHA256",
                "${{ inputs.manifest_sha256 }}",
            )],
        ),
    ]);
    let call = jobs::local_action_with_inputs(
        job.action,
        job.name,
        action_steps,
        vec![
            (
                "artifact_id",
                "Artifact ID from this target's build job",
                &format!("${{{{ needs.{}.outputs.artifact_id }}}}", job.build_job),
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
    let mut fields = with_permissions(
        workflow_steps::with_needs(
            base(job.name, job.runs_on, 120),
            &[job.build_job, "candidate-manifest"],
        ),
        workflow_steps::qualification_permissions(),
    );
    fields.retain(|(key, _)| key != "name");
    Ok(finish(job.id, fields, vec![job.source_step.clone(), call]))
}
