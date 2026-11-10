// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;

use super::super::features::{base, finish};
use super::assets::ProductAsset;
use super::workflow_steps::{self, with_permissions};
use super::{ProductReleasePins, assets, jobs, manifest};

#[path = "schema2_generator_release_homebrew_qualification.rs"]
mod homebrew;

pub(super) fn homebrew_job(
    pins: &ProductReleasePins,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    homebrew::job(pins, actions)
}

pub(super) struct QualificationJob<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub action: &'a str,
    pub runs_on: Yaml,
    pub build_job: &'a str,
    pub product: ProductAsset,
    pub source_step: &'a Yaml,
}

pub(super) fn job(
    job: QualificationJob<'_>,
    pins: &ProductReleasePins,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let mut action_steps = vec![
        workflow_steps::mise_step(pins.setup_for(job.product.target))?,
        workflow_steps::command_step(
            "Install pinned qualification tools",
            &pins.install_qualify_tools_argv,
        )?,
        workflow_steps::verify_rust_step(pins)?,
        workflow_steps::command_step("Verify jactionlint", &pins.verify_lint_argv)?,
    ];
    action_steps.extend(source_steps(job.product, pins));
    action_steps.extend(assets::download_build_steps(
        job.product,
        "Download built asset files",
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
    let source_names = [
        "artifact_id",
        "sources_id",
        "sources_digest",
        "sources_tree",
        "sources_root_lock",
        "sources_runner_lock",
        "sources_attempt",
    ];
    let source_values =
        source_names.map(|name| format!("${{{{ needs.{}.outputs.{name} }}}}", job.build_job));
    let mut inputs: Vec<_> = source_names
        .iter()
        .zip(source_values.iter())
        .map(|(name, value)| (*name, "Producer artifact identity", value.as_str()))
        .collect();
    inputs.extend([
        (
            "manifest_artifact_id",
            "Canonical manifest artifact ID",
            "${{ needs.candidate-manifest.outputs.artifact_id }}",
        ),
        (
            "manifest_sha256",
            "Canonical manifest SHA-256",
            "${{ needs.candidate-manifest.outputs.manifest_sha256 }}",
        ),
    ]);
    let call = jobs::local_action_with_inputs(job.action, job.name, action_steps, inputs, actions)?;
    let mut fields = with_permissions(
        workflow_steps::with_needs(
            base(job.name, job.runs_on, JobTimeout::PRODUCT_QUALIFICATION),
            &[job.build_job, "candidate-manifest"],
        ),
        workflow_steps::qualification_permissions(),
    );
    fields.retain(|(key, _)| key != "name");
    Ok(finish(job.id, fields, vec![job.source_step.clone(), call]))
}

pub(super) fn source_steps(product: ProductAsset, pins: &ProductReleasePins) -> Vec<Yaml> {
    let mut admission = workflow_steps::bash_step_with_env(
        "Admit exact same-run Cargo source artifact",
        "/usr/bin/python3 -I -S scripts/generator-release/stable-source-admission.py",
        vec![
            ("GH_TOKEN", "${{ github.token }}"),
            ("SOURCES_ID", "${{ inputs.sources_id }}"),
            ("SOURCES_DIGEST", "${{ inputs.sources_digest }}"),
            ("SOURCES_TREE", "${{ inputs.sources_tree }}"),
            ("SOURCES_ROOT_LOCK", "${{ inputs.sources_root_lock }}"),
            ("SOURCES_RUNNER_LOCK", "${{ inputs.sources_runner_lock }}"),
            ("SOURCES_ATTEMPT", "${{ inputs.sources_attempt }}"),
            ("SOURCES_NAME", product.workflow_artifact),
        ],
    );
    if let Yaml::Map(fields) = &mut admission {
        fields.push(("id".to_owned(), Yaml::str("sources-admission")));
    }
    let mut download = workflow_steps::download_step_by_id(
        "Download admitted Cargo sources",
        "${{ inputs.sources_id }}",
        "${{ runner.temp }}/stable-cargo-download",
    );
    if let Yaml::Map(fields) = &mut download {
        fields.push(("id".to_owned(), Yaml::str("download-sources")));
        fields.push((
            "if".to_owned(),
            Yaml::str("${{ steps.sources-admission.outputs.admitted == 'true' }}"),
        ));
        fields.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    vec![
        admission,
        download,
        workflow_steps::bash_step_with_env(
            "Prepare complete locked Cargo sources",
            "bash scripts/generator-release/stable-source-consumer.sh",
            vec![
                ("SOURCES_TREE", "${{ inputs.sources_tree }}"),
                (
                    "SOURCES_DOWNLOADED",
                    "${{ steps.download-sources.outcome }}",
                ),
                ("VELNOR_RUST_VERSION", &pins.rust_version),
            ],
        ),
    ]
}
