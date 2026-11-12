// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn source_preview_family_pins_merged_runtime_authority() -> Result<(), Box<dyn Error>> {
    let mut request = request()?;
    request.runner_image_family = Some(
        velnor_actions_contract::config::MacRunnerQualificationConfig {
            runner_id: 2,
            companion_workflow_path: ".github/workflows/qualify-runner-family.yml".to_owned(),
            companion_workflow_ref: "refs/heads/main".to_owned(),
            companion_workflow_sha: "c".repeat(40),
            docker_socket: "unix:///tmp/fixture.sock".to_owned(),
            state_dir: "/tmp/fixture-state".to_owned(),
            sdk: velnor_actions_contract::config::MacSdkSelection::SourcePreview {},
        },
    );
    let files = render_schema2_workflows(&request)?;
    let workflow: serde_json::Value = serde_saphyr::from_str(find_file(
        &files,
        ".github/workflows/product-release-images.yml",
    )?)?;
    let proof = &workflow["jobs"]["verify-mac-proof"];
    assert_eq!(
        proof["uses"],
        "tailrocks/velnor-new/.github/workflows/source-preview-mac-proof.yml@533d225c08785adc4367340e1db4dc111ee2b5a2"
    );
    assert_eq!(
        proof["needs"],
        serde_json::json!(["discover-producer", "publish-images"])
    );
    assert_eq!(
        proof["permissions"],
        serde_json::json!({"actions": "read", "contents": "read"})
    );
    assert_eq!(proof["with"]["public_source"], "${{ github.sha }}");
    assert_eq!(proof["with"]["public_run_id"], "${{ github.run_id }}");
    assert_eq!(
        proof["with"]["public_run_attempt"],
        "${{ github.run_attempt }}"
    );
    Ok(())
}
