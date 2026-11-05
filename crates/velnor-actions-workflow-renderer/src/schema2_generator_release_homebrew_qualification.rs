// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::{JobTimeout, ReleaseTarget};

use super::super::super::features::{base, finish};
use super::super::{ProductReleasePins, assets, jobs, manifest, workflow_steps};

const RUNS_ON: &str = "macos-15";
const JOB_TIMEOUT_MINUTES: u16 = 30;
const ADMISSION_DIR: &str = "homebrew-admission";
const RESULT_DIR: &str = "homebrew-result";
const ADMISSION_ZIP_DIR: &str = "homebrew-admission-zip";
const RESULT_ZIP_DIR: &str = "homebrew-result-zip";

fn download_raw_step(name: &str, artifact_id: &str, path: &str) -> Yaml {
    let mut step = workflow_steps::download_step_by_id(name, artifact_id, path);
    if let Yaml::Map(fields) = &mut step {
        for (key, value) in fields {
            if key == "with" {
                if let Yaml::Map(inputs) = value {
                    inputs.push(("skip-decompress".to_owned(), Yaml::Bool(true)));
                }
                break;
            }
        }
    }
    step
}

pub(super) fn admission_job(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    linux_job(pins, false)
}

pub(super) fn finalization_job(pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    linux_job(pins, true)
}

fn linux_job(pins: &ProductReleasePins, finalizing: bool) -> Result<(String, Yaml), RenderError> {
    let product = assets::LINUX;
    let id = if finalizing {
        "qualify-homebrew-finalize"
    } else {
        "qualify-homebrew-admit"
    };
    let mut fields = workflow_steps::with_permissions(
        base(
            id,
            Yaml::str(crate::release_tree::RELEASE_HOSTED_RUNNER),
            JobTimeout::new(JOB_TIMEOUT_MINUTES).map_err(RenderError::Contract)?,
        ),
        workflow_steps::perm(&[
            ("actions", "read"),
            ("contents", "read"),
            ("attestations", "read"),
        ]),
    );
    let needs = if finalizing {
        vec![
            product.build_job,
            "qualify-linux",
            "candidate-manifest",
            "qualify-homebrew-admit",
            "qualify-homebrew-macos",
        ]
    } else {
        vec![product.build_job, "qualify-linux", "candidate-manifest"]
    };
    fields = workflow_steps::with_needs(fields, &needs);
    if !finalizing {
        fields.push(("outputs".to_owned(), artifact_outputs("admission_sha256")));
    }
    Ok(finish(id, fields, linux_steps(pins, finalizing)?))
}

fn linux_steps(pins: &ProductReleasePins, finalizing: bool) -> Result<Vec<Yaml>, RenderError> {
    let product = assets::LINUX;
    let mut steps = vec![
        start_budget_step(),
        workflow_steps::checkout_step(),
        workflow_steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
        workflow_steps::install_gh_step(&pins.install_gh_argv)?,
    ];
    steps.extend(assets::download_build_steps_for_id(
        product,
        "Download exact Linux candidate",
        "${{ needs.build-linux.outputs.artifact_id }}",
    ));
    steps.extend([
        workflow_steps::validate_artifact_id("${{ needs.candidate-manifest.outputs.artifact_id }}"),
        workflow_steps::download_step_by_id(
            "Download canonical candidate manifest",
            "${{ needs.candidate-manifest.outputs.artifact_id }}",
            manifest::DIR,
        ),
        workflow_steps::bash_step(
            "Verify Linux candidate provenance",
            &assets::verify_provenance_script(product, pins),
        ),
        workflow_steps::bash_step(
            "Verify Linux candidate checksum",
            &format!(
                "set -eu\ncd {}\n{} {}",
                product.directory, product.checksum_command, product.sidecar
            ),
        ),
    ]);
    if finalizing {
        steps.extend([
            workflow_steps::validate_artifact_id(
                "${{ needs.qualify-homebrew-admit.outputs.artifact_id }}",
            ),
            download_raw_step(
                "Download exact admission",
                "${{ needs.qualify-homebrew-admit.outputs.artifact_id }}",
                ADMISSION_ZIP_DIR,
            ),
            workflow_steps::validate_artifact_id(
                "${{ needs.qualify-homebrew-macos.outputs.artifact_id }}",
            ),
            download_raw_step(
                "Download exact Mac result",
                "${{ needs.qualify-homebrew-macos.outputs.artifact_id }}",
                RESULT_ZIP_DIR,
            ),
        ]);
    }
    steps.push(linux_operation_step(finalizing));
    if !finalizing {
        steps.extend(evidence_steps(
            ADMISSION_DIR,
            "admission",
            "admission_sha256",
            "sha256sum",
        ));
    }
    Ok(steps)
}

fn linux_operation_step(finalizing: bool) -> Yaml {
    let product = assets::LINUX;
    let operation = if finalizing {
        "finalize-homebrew-qualification-v1"
    } else {
        "admit-homebrew-qualification-v1"
    };
    let mut env = vec![
        ("GH_TOKEN", "${{ github.token }}"),
        ("VELNOR_INTERNAL_OP", operation),
        (
            "VELNOR_RELEASE_MANIFEST_SHA256",
            "${{ needs.candidate-manifest.outputs.manifest_sha256 }}",
        ),
    ];
    if finalizing {
        env.extend([
            (
                "VELNOR_HOME_ADMISSION_SHA256",
                "${{ needs.qualify-homebrew-admit.outputs.admission_sha256 }}",
            ),
            (
                "VELNOR_HOME_RESULT_SHA256",
                "${{ needs.qualify-homebrew-macos.outputs.result_sha256 }}",
            ),
            (
                "VELNOR_HOME_ADMISSION_ARCHIVE_SHA256",
                "${{ needs.qualify-homebrew-admit.outputs.artifact_digest }}",
            ),
            (
                "VELNOR_HOME_RESULT_ARCHIVE_SHA256",
                "${{ needs.qualify-homebrew-macos.outputs.artifact_digest }}",
            ),
            (
                "VELNOR_HOME_ADMISSION_ARCHIVE_SIZE",
                "${{ needs.qualify-homebrew-admit.outputs.artifact_size }}",
            ),
            (
                "VELNOR_HOME_RESULT_SIZE",
                "${{ needs.qualify-homebrew-macos.outputs.artifact_size }}",
            ),
        ]);
    }
    workflow_steps::bash_step_with_env(
        "Verify historical Homebrew qualification",
        &format!(
            "set -eu\nchmod 0500 ./{}/{}\n./{}/{}",
            product.directory, product.binary, product.directory, product.binary
        ),
        env,
    )
}

fn artifact_outputs(digest: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "artifact_id".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
        ),
        (
            "artifact_digest".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-digest }}"),
        ),
        (
            "artifact_size".to_owned(),
            Yaml::str("${{ steps.evidence_digest.outputs.artifact_size }}"),
        ),
        (
            digest.to_owned(),
            Yaml::str(format!("${{{{ steps.evidence_digest.outputs.{digest} }}}}")),
        ),
    ])
}

fn evidence_steps(directory: &str, file: &str, digest: &str, hash: &str) -> Vec<Yaml> {
    let upload_path = if file == "admission" {
        format!("{directory}/admission.zip")
    } else {
        format!("{directory}/result.json")
    };
    let mut upload = workflow_steps::upload_step_with_id(
        "upload",
        "Upload bound evidence",
        directory,
        &[&upload_path],
    );
    if let Yaml::Map(fields) = &mut upload {
        for (key, value) in fields {
            if key == "with" {
                if let Yaml::Map(inputs) = value {
                    inputs.push(("archive".to_owned(), Yaml::Bool(false)));
                }
                break;
            }
        }
    }
    vec![
        workflow_steps::bash_step_with_id(
            "evidence_digest",
            "Bind evidence bytes",
            &format!(
                "set -eu\ntest -f {directory}/{file}.json && test ! -L {directory}/{file}.json\ndigest=\"$({hash} {directory}/{file}.json | awk 'NR == 1 {{print $1}}')\"\n[[ \"$digest\" =~ ^[0-9a-f]{{64}}$ ]]\nprintf '{digest}=%s\\n' \"$digest\" >> \"$GITHUB_OUTPUT\"\ntest -f {upload_path} && test ! -L {upload_path}\nsize=\"$(wc -c < {upload_path} | tr -d ' ')\"\n[[ \"$size\" =~ ^[1-9][0-9]*$ ]]\nprintf 'artifact_size=%s\\n' \"$size\" >> \"$GITHUB_OUTPUT\""
            ),
        ),
        upload,
    ]
}

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
            (
                "admission_artifact_id",
                "Exact same-run admission artifact ID",
                "${{ needs.qualify-homebrew-admit.outputs.artifact_id }}",
            ),
            (
                "admission_archive_sha256",
                "SHA-256 of the uploaded admission archive",
                "${{ needs.qualify-homebrew-admit.outputs.artifact_digest }}",
            ),
            (
                "admission_archive_size",
                "Byte size of the uploaded admission archive",
                "${{ needs.qualify-homebrew-admit.outputs.artifact_size }}",
            ),
            (
                "admission_sha256",
                "SHA-256 of authenticated admission bytes",
                "${{ needs.qualify-homebrew-admit.outputs.admission_sha256 }}",
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
        workflow_steps::perm(&[("contents", "read")]),
    );
    fields.retain(|(key, _)| key != "name");
    fields = workflow_steps::with_needs(
        fields,
        &[
            product.build_job,
            "candidate-manifest",
            "qualify-homebrew-admit",
        ],
    );
    fields.push(("outputs".to_owned(), artifact_outputs("result_sha256")));
    let mut steps = vec![start_budget_step(), workflow_steps::checkout_step(), call];
    steps.extend(evidence_steps(
        RESULT_DIR,
        "result",
        "result_sha256",
        "shasum -a 256",
    ));
    Ok(finish("qualify-homebrew-macos", fields, steps))
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
    ];
    steps.extend(assets::download_build_steps_for_id(
        product,
        "Download exact ARM candidate artifact",
        "${{ inputs.artifact_id }}",
    ));
    steps.push(workflow_steps::validate_artifact_id(
        "${{ inputs.admission_artifact_id }}",
    ));
    steps.push(download_raw_step(
        "Download exact admission",
        "${{ inputs.admission_artifact_id }}",
        ADMISSION_ZIP_DIR,
    ));
    steps.extend([
        workflow_steps::validate_artifact_id("${{ inputs.manifest_artifact_id }}"),
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
            "Validate admission; install and test preserved Homebrew kegs",
            &format!("set -eu\nenv -i PATH=\"$PATH\" HOME=\"$HOME\" TMPDIR=\"${{TMPDIR:-/tmp}}\" RUNNER_TEMP=\"$RUNNER_TEMP\" RUNNER_OS=\"$RUNNER_OS\" RUNNER_ARCH=\"$RUNNER_ARCH\" RUNNER_ENVIRONMENT=\"$RUNNER_ENVIRONMENT\" GITHUB_ACTIONS=\"$GITHUB_ACTIONS\" GITHUB_REPOSITORY=\"$GITHUB_REPOSITORY\" GITHUB_REPOSITORY_ID=\"$GITHUB_REPOSITORY_ID\" GITHUB_RUN_ID=\"$GITHUB_RUN_ID\" GITHUB_RUN_ATTEMPT=\"$GITHUB_RUN_ATTEMPT\" GITHUB_WORKFLOW_REF=\"$GITHUB_WORKFLOW_REF\" GITHUB_WORKFLOW_SHA=\"$GITHUB_WORKFLOW_SHA\" GITHUB_SHA=\"$GITHUB_SHA\" GITHUB_WORKSPACE=\"$GITHUB_WORKSPACE\" VELNOR_HOME_BUDGET_DEADLINE_EPOCH=\"$VELNOR_HOME_BUDGET_DEADLINE_EPOCH\" VELNOR_HOME_ADMISSION_SHA256=\"$VELNOR_HOME_ADMISSION_SHA256\" VELNOR_HOME_ADMISSION_ARCHIVE_SHA256=\"$VELNOR_HOME_ADMISSION_ARCHIVE_SHA256\" VELNOR_HOME_ADMISSION_ARCHIVE_SIZE=\"$VELNOR_HOME_ADMISSION_ARCHIVE_SIZE\" VELNOR_RELEASE_MANIFEST_SHA256=\"$VELNOR_RELEASE_MANIFEST_SHA256\" VELNOR_INTERNAL_OP=qualify-homebrew-v1 {candidate_path}"),
            vec![
                ("VELNOR_HOME_ADMISSION_SHA256", "${{ inputs.admission_sha256 }}"),
                ("VELNOR_HOME_ADMISSION_ARCHIVE_SHA256", "${{ inputs.admission_archive_sha256 }}"),
                ("VELNOR_HOME_ADMISSION_ARCHIVE_SIZE", "${{ inputs.admission_archive_size }}"),
                ("BASH_ENV", ""),
                ("ENV", ""),
                ("LD_PRELOAD", ""),
                ("LD_LIBRARY_PATH", ""),
                ("LD_AUDIT", ""),
                ("DYLD_INSERT_LIBRARIES", ""),
                ("DYLD_LIBRARY_PATH", ""),
                ("DYLD_FRAMEWORK_PATH", ""),
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
