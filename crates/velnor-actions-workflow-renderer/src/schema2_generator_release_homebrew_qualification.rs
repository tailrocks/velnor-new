// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::{JobTimeout, ReleaseTarget};

use super::super::super::features::{base, finish};
use super::super::{ProductReleasePins, assets, jobs, manifest, workflow_steps};

const RUNS_ON: &str = "macos-15";
const JOB_TIMEOUT_MINUTES: u16 = 30;

pub(super) fn job(
    pins: &ProductReleasePins,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let product = assets::MACOS_ARM64;
    let candidate_path = format!("{}/{}", product.directory, product.binary);
    let action_steps = candidate_steps(pins, &candidate_path)?;
    let call = jobs::local_action_with_inputs(
        "generator-release-qualify-homebrew",
        "Qualify historical Homebrew releases",
        action_steps,
        vec![
            (
                "artifact_id",
                "Artifact ID from the macOS ARM build job",
                "${{ needs.build-macos.outputs.artifact_id }}",
            ),
            (
                "manifest_artifact_id",
                "Artifact ID of the canonical same-run candidate manifest",
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
    let timeout = JobTimeout::new(JOB_TIMEOUT_MINUTES).map_err(RenderError::Contract)?;
    let mut fields = workflow_steps::with_permissions(
        base(
            "Qualify Homebrew A/D on hosted ARM macOS",
            Yaml::str(RUNS_ON),
            timeout,
        ),
        workflow_steps::perm(&[("contents", "read"), ("attestations", "read")]),
    );
    fields.retain(|(key, _)| key != "name");
    fields = workflow_steps::with_needs(fields, &[product.build_job, "candidate-manifest"]);
    Ok(finish(
        "qualify-homebrew-macos",
        fields,
        vec![start_budget_step(), workflow_steps::checkout_step(), call],
    ))
}

fn candidate_steps(
    pins: &ProductReleasePins,
    candidate_path: &str,
) -> Result<Vec<Yaml>, RenderError> {
    let product = assets::MACOS_ARM64;
    let mut steps = vec![
        workflow_steps::mise_step(pins.setup_for(ReleaseTarget::MacosArm64))?,
        workflow_steps::command_step(
            "Install pinned qualification tools",
            &pins.install_qualify_tools_argv,
        )?,
        workflow_steps::verify_rust_step(pins)?,
        workflow_steps::command_step("Verify jactionlint", &pins.verify_lint_argv)?,
        workflow_steps::install_gh_step(&pins.install_gh_argv)?,
    ];
    steps.extend(assets::download_build_steps_for_id(
        product,
        "Download exact ARM candidate artifact",
        "${{ inputs.artifact_id }}",
    ));
    steps.extend([
        workflow_steps::download_step_by_id(
            "Download canonical same-run candidate manifest",
            "${{ inputs.manifest_artifact_id }}",
            manifest::DIR,
        ),
        workflow_steps::bash_step(
            "Verify source-bound build record",
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
            "Verify candidate release identity without credentials",
            &assets::qualification_script(product.binary, product.directory),
            vec![(
                "VELNOR_RELEASE_MANIFEST_SHA256",
                "${{ inputs.manifest_sha256 }}",
            )],
        ),
        workflow_steps::bash_step_with_env(
            "Authenticate historical A and D; install and test preserved Homebrew kegs",
            &format!("set -eu\n{candidate_path}"),
            vec![
                ("GH_TOKEN", "${{ github.token }}"),
                ("VELNOR_INTERNAL_OP", "qualify-homebrew-v1"),
                (
                    "VELNOR_RELEASE_MANIFEST_SHA256",
                    "${{ inputs.manifest_sha256 }}",
                ),
            ],
        ),
    ]);
    Ok(steps)
}

fn start_budget_step() -> Yaml {
    workflow_steps::bash_step(
        "Start shared Homebrew qualification budget",
        &format!(
            "set -eu\nnow=\"$(date +%s)\"\nprintf 'VELNOR_HOME_BUDGET_DEADLINE_EPOCH=%s\\n' \"$((now + {JOB_TIMEOUT_MINUTES} * 60))\" >> \"$GITHUB_ENV\""
        ),
    )
}
