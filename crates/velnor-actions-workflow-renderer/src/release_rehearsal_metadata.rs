// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{base_job, map, step};
use crate::{RenderError, yaml::Yaml};

pub(super) fn metadata_job(
    config: &velnor_actions_contract::config::ReleaseRehearsalMetadataConfig,
) -> Result<Yaml, RenderError> {
    use crate::verifier_builders::{ToolchainPhase, rust_verifier_shell_step};
    use velnor_actions_mise::{PinnedTool, ToolCatalog};
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Rust, &config.rust_version)
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    if velnor_actions_mise::catalog::rust_channel_manifest_sha256(&config.rust_version).is_none() {
        return Err(RenderError::InvalidWorkflow(
            "metadata_rust_unpinned".to_owned(),
        ));
    }
    let mise = crate::setup::MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: velnor_actions_mise::MISE_VERSION.to_owned(),
        sha256: velnor_actions_mise::MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    let setup = crate::setup::mise_setup_step(&mise)?;
    let prepare = crate::release_gates::release_rust_prepare(&catalog)?;
    let verify = rust_verifier_shell_step(&catalog, ToolchainPhase::Installed)?;
    let yaml = |step: &velnor_actions_contract::Step| {
        crate::document_steps::non_internal_step_to_yaml(
            "metadata",
            step,
            &std::collections::BTreeMap::new(),
            "ubuntu-24.04",
        )
    };
    let download = map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::DOWNLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("approved-source")),
                ("path", Yaml::str("${{ runner.temp }}/approved")),
            ]),
        ),
    ]);
    let metadata = metadata_run_step(config);
    let upload = metadata_upload_step();
    let mut job = base_job(
        vec![
            download,
            yaml(&setup)?,
            yaml(&prepare)?,
            yaml(&verify)?,
            metadata,
            yaml(&verify)?,
            upload,
        ],
        map(vec![]),
    );
    job.push(("needs", Yaml::str("gate")));
    Ok(map(job))
}

fn metadata_run_step(
    config: &velnor_actions_contract::config::ReleaseRehearsalMetadataConfig,
) -> Yaml {
    let script = include_str!("release_rehearsal_metadata.sh")
        .replace(
            "__VELNOR_EXTRACT_PYTHON__",
            &include_str!("release_rehearsal_extract.py").replace(
                "__VELNOR_PATHS_PYTHON__",
                include_str!("release_rehearsal_paths.py"),
            ),
        )
        .replace(
            "__VELNOR_METADATA_PYTHON__",
            include_str!("release_rehearsal_metadata.py"),
        );
    step(
        "Compute approved source metadata",
        &script,
        map(vec![
            ("SOURCE_SHA", Yaml::str("${{ inputs.source_sha }}")),
            (
                "APPROVED_SHA256",
                Yaml::str("${{ needs.gate.outputs.approved_sha256 }}"),
            ),
            ("METADATA_MANIFEST", Yaml::str(&config.manifest_path)),
            ("RUST_VERSION", Yaml::str(&config.rust_version)),
        ]),
    )
}

fn metadata_upload_step() -> Yaml {
    map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::UPLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("approved-metadata-untrusted")),
                (
                    "path",
                    Yaml::str("${{ runner.temp }}/metadata-scratch/metadata.json"),
                ),
                ("if-no-files-found", Yaml::str("error")),
                ("retention-days", Yaml::Int(1)),
            ]),
        ),
    ])
}

pub(super) fn append_metadata_witness_steps(
    steps: &mut Vec<Yaml>,
    metadata: &velnor_actions_contract::config::ReleaseRehearsalMetadataConfig,
) {
    steps.push(map(vec![
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::DOWNLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("approved-metadata-untrusted")),
                ("path", Yaml::str("${{ runner.temp }}/metadata")),
            ]),
        ),
    ]));
    steps.push(step("Validate transferred metadata", "set -euo pipefail\np=\"$RUNNER_TEMP/metadata/metadata.json\"\ntest ! -L \"$p\" && test -f \"$p\"\ntest \"$(wc -c < \"$p\")\" -le 16777216\n", map(vec![])));
    steps.push(map(vec![
        ("id", Yaml::str("metadata_upload")),
        (
            "uses",
            Yaml::str(velnor_actions_actionlint::actions::UPLOAD_ARTIFACT_ACTION_USES),
        ),
        (
            "with",
            map(vec![
                ("name", Yaml::str("approved-metadata-observation")),
                (
                    "path",
                    Yaml::str("${{ runner.temp }}/metadata/metadata.json"),
                ),
                ("if-no-files-found", Yaml::str("error")),
                ("retention-days", Yaml::Int(1)),
            ]),
        ),
    ]));
    steps.push(step(
        "Bind metadata artifact",
        &include_str!("release_rehearsal_artifact_marker.sh")
            .replace("VELNOR_REHEARSAL_WITNESS", "VELNOR_REHEARSAL_METADATA"),
        map(vec![
            (
                "VELNOR_ARTIFACT_ID",
                Yaml::str("${{ steps.metadata_upload.outputs.artifact-id }}"),
            ),
            (
                "VELNOR_ARTIFACT_DIGEST",
                Yaml::str("${{ steps.metadata_upload.outputs.artifact-digest }}"),
            ),
        ]),
    ));
    steps.push(step("Bind approved metadata source", "set -euo pipefail\n[[ \"$SOURCE_SHA\" =~ ^[a-f0-9]{40}$ && \"$APPROVED_TREE\" =~ ^[a-f0-9]{40}$ && \"$APPROVED_SHA256\" =~ ^[a-f0-9]{64}$ ]]\nprintf 'VELNOR_METADATA_SOURCE run=%s attempt=%s source=%s config=%s tree=%s sha256=%s\\n' \"$GITHUB_RUN_ID\" \"$GITHUB_RUN_ATTEMPT\" \"$SOURCE_SHA\" \"$INTENT_CONFIG_PATH\" \"$APPROVED_TREE\" \"$APPROVED_SHA256\"\n", map(vec![("SOURCE_SHA", Yaml::str("${{ inputs.source_sha }}")), ("APPROVED_SHA256", Yaml::str("${{ needs.gate.outputs.approved_sha256 }}")), ("APPROVED_TREE", Yaml::str("${{ needs.gate.outputs.approved_tree }}")), ("INTENT_CONFIG_PATH", Yaml::str(&metadata.intent_config_path)), ("HISTORICAL_MANIFEST_VERSION", Yaml::str(&metadata.qualification.historical_manifest.version)), ("HISTORICAL_MANIFEST_SOURCE", Yaml::str(&metadata.qualification.historical_manifest.source_sha)), ("HISTORICAL_MANIFEST_SHA256", Yaml::str(&metadata.qualification.historical_manifest.sha256)),
    ("CANDIDATE_REPOSITORY", Yaml::str(&metadata.qualification.candidate_repository)),
    ("CANDIDATE_REPOSITORY_ID", Yaml::str(metadata.qualification.candidate_repository_id.to_string())),
    ("CANDIDATE_DEFAULT_BRANCH", Yaml::str(&metadata.qualification.candidate_default_branch)),
    ("CANDIDATE_REQUIRED_WORKFLOW", Yaml::str(&metadata.qualification.candidate_required_workflow_path)),
    ("COMPLETION_APP_ID", Yaml::str(metadata.qualification.completion_app_id.to_string())),
    ("COMPLETION_CHECK_NAME", Yaml::str(&metadata.qualification.completion_check_name)),
    ("REVIEWER_APP_IDS", Yaml::str(metadata.qualification.reviewer_app_ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",")))])));
}
