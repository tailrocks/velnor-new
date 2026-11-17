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
                "sources_id",
                "Source artifact ID",
                "${{ needs.build-macos.outputs.sources_id }}",
            ),
            (
                "sources_digest",
                "Source artifact digest",
                "${{ needs.build-macos.outputs.sources_digest }}",
            ),
            (
                "sources_tree",
                "Source tree digest",
                "${{ needs.build-macos.outputs.sources_tree }}",
            ),
            (
                "sources_root_lock",
                "Root lock digest",
                "${{ needs.build-macos.outputs.sources_root_lock }}",
            ),
            (
                "sources_runner_lock",
                "Runner lock digest",
                "${{ needs.build-macos.outputs.sources_runner_lock }}",
            ),
            (
                "sources_attempt",
                "Source producer attempt",
                "${{ needs.build-macos.outputs.sources_attempt }}",
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
        workflow_steps::perm(&[
            ("actions", "read"),
            ("contents", "read"),
            ("attestations", "read"),
        ]),
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
        workflow_steps::command_step("Install pinned evidence transport", &pins.install_gh_argv)?,
        workflow_steps::bash_step_with_env(
            "Acquire bounded historical Homebrew evidence",
            &acquisition_script(pins)?,
            vec![
                ("GH_TOKEN", "${{ github.token }}"),
                (
                    "VELNOR_HOMEBREW_EVIDENCE_DIR",
                    "${{ runner.temp }}/velnor-homebrew-evidence",
                ),
            ],
        ),
        workflow_steps::verify_rust_step(pins)?,
        workflow_steps::command_step("Verify jactionlint", &pins.verify_lint_argv)?,
    ];
    steps.extend(super::source_steps(product, pins)?);
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
            &format!(
                "set -eu\n{}",
                workflow_steps::credential_free_command(candidate_path)
            ),
            vec![
                (
                    "VELNOR_HOMEBREW_EVIDENCE_DIR",
                    "${{ runner.temp }}/velnor-homebrew-evidence",
                ),
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

fn acquisition_script(pins: &ProductReleasePins) -> Result<String, RenderError> {
    let bun = pins
        .install_qualify_tools_argv
        .iter()
        .find(|argument| argument.starts_with("bun@"))
        .ok_or_else(|| RenderError::BadCommand("homebrew_acquisition_bun_pin".to_owned()))?;
    let command = crate::commands::join_argv_for_run(&[
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        bun.clone(),
        "--".to_owned(),
        "bun".to_owned(),
        "--no-env-file".to_owned(),
        "--no-install".to_owned(),
        "--config=/dev/null".to_owned(),
        "acquire.ts".to_owned(),
    ])?;
    let gh = workflow_steps::gh_function(&pins.gh_argv)?;
    let catalog = format!(
        "[{},{}]",
        include_str!("../../../.velnor/qualification/homebrew/preview-a.json"),
        include_str!("../../../.velnor/qualification/homebrew/preview-d.json")
    );
    let literal = serde_json::to_string(&catalog)
        .map_err(|error| RenderError::BadCommand(error.to_string()))?;
    let source = include_str!("homebrew_qualification_acquire.ts")
        .replace("\"__VELNOR_HOMEBREW_CATALOG__\"", &literal);
    Ok(format!(
        "set -eu\numask 077\nunset GH_HOST GH_CONFIG_DIR BUN_OPTIONS NODE_OPTIONS\nprivate=\"$(mktemp -d \"$RUNNER_TEMP/velnor-homebrew-acquire.XXXXXX\")\"\ntrap 'rm -rf \"$private\"' EXIT\ncat > \"$private/acquire.ts\" <<'VELNOR_HOMEBREW_ACQUIRE_TS'\n{source}\nVELNOR_HOMEBREW_ACQUIRE_TS\ncd \"$private\"\nexport MISE_DATA_DIR=\"${{MISE_DATA_DIR:-$HOME/.local/share/mise}}\"\nexport HOME=\"$private\"\n{command}\n{gh}\n(ulimit -f 4096; gh attestation trusted-root > \"$VELNOR_HOMEBREW_EVIDENCE_DIR/trusted_root.jsonl\")\ntest -s \"$VELNOR_HOMEBREW_EVIDENCE_DIR/trusted_root.jsonl\"\ntest \"$(wc -c < \"$VELNOR_HOMEBREW_EVIDENCE_DIR/trusted_root.jsonl\")\" -le 4194304"
    ))
}
