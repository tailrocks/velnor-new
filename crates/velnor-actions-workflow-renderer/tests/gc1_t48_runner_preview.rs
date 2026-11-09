// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn source_preview_family_rejects_missing_runtime_authority() -> Result<(), Box<dyn Error>> {
    let mut request = request()?;
    request.runner_image_family = Some(
        velnor_actions_contract::config::MacRunnerQualificationConfig {
            runner_id: 21,
            companion_workflow_path: ".github/workflows/qualify-runner-family.yml".to_owned(),
            companion_workflow_ref: "refs/heads/main".to_owned(),
            companion_workflow_sha: "c".repeat(40),
            docker_socket: "unix:///tmp/fixture.sock".to_owned(),
            state_dir: "/tmp/fixture-state".to_owned(),
            sdk: velnor_actions_contract::config::MacSdkSelection::SourcePreview {},
        },
    );
    let error = render_schema2_workflows(&request)
        .err()
        .ok_or("preview rendered without Q authority")?;
    assert!(
        error
            .to_string()
            .contains("preview_runtime_authority_unavailable")
    );
    Ok(())
}
