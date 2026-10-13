use super::action_snapshots::{Actions, action};

pub(super) fn assert_candidate_qualification(
    workflow: &str,
    actions: &Actions,
    job_id: &str,
    action_name: &str,
    build_job: &str,
    directory: &str,
    binary: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let action_body = action(actions, action_name)?;
    let archive_extract = action_body
        .find("Prevalidate and extract uploaded candidate archive")
        .ok_or("missing prevalidated candidate archive extraction")?;
    let file_modes = action_body
        .find("Verify extracted candidate file types and executable mode")
        .ok_or("missing extracted candidate file-type check")?;
    let checksum = action_body
        .find("Verify downloaded checksum sidecar")
        .ok_or("missing downloaded checksum verification")?;
    let provenance = action_body
        .find("Verify candidate provenance record")
        .ok_or("missing candidate provenance verification")?;
    let qualification = action_body
        .find("Qualify downloaded candidate")
        .ok_or("missing exact candidate qualification")?;
    let job = super::super::job_body(workflow, job_id)?;
    let caller_artifact_id =
        format!("artifact_id: ${{{{ needs.{build_job}.outputs.artifact_id }}}}");
    assert!(
        archive_extract < file_modes
            && file_modes < provenance
            && provenance < checksum
            && checksum < qualification,
        "{job}"
    );
    assert!(job.contains(&caller_artifact_id), "{job}");
    assert!(
        job.contains("manifest_sha256: ${{ needs.candidate-manifest.outputs.manifest_sha256 }}"),
        "{job}"
    );
    assert!(
        action_body.contains("artifact-ids: ${{ inputs.artifact_id }}"),
        "{action_body}"
    );
    assert!(
        action_body.contains("inputs:\n  artifact_id:"),
        "{action_body}"
    );
    assert!(
        action_body.contains("  manifest_artifact_id:")
            && action_body.contains("  manifest_sha256:")
            && action_body
                .contains("VELNOR_RELEASE_MANIFEST_SHA256: ${{ inputs.manifest_sha256 }}"),
        "{action_body}"
    );
    assert!(action_body.contains("required: true"), "{action_body}");
    assert!(!action_body.contains("${{ needs."), "{action_body}");
    assert!(
        !action_body.contains("Attest built artifacts"),
        "{action_body}"
    );
    assert!(!action_body.contains("GH_TOKEN:"), "{action_body}");
    assert!(
        !action_body.contains("actions/upload-artifact@"),
        "{action_body}"
    );
    assert_eq!(
        action_body.matches("Qualify downloaded candidate").count(),
        1
    );
    assert!(action_body.contains("test -x "), "{action_body}");
    assert!(action_body.contains("toolchain"), "{action_body}");
    assert!(action_body.contains("--version"), "{action_body}");
    assert!(
        action_body.contains(&format!(
            "scripts/capture-opentofu-goldens.sh check-release \\\"$GITHUB_WORKSPACE/{directory}/{binary}\\\" \\\"$GITHUB_WORKSPACE/manifest-assets/release-manifest.json\\\" \\\"$VELNOR_RELEASE_MANIFEST_SHA256\\\""
        )),
        "{action_body}"
    );
    assert_release_fixture_generation_requirements();
    Ok(())
}

fn assert_release_fixture_generation_requirements() {
    let fixture_check = include_str!("../../../scripts/capture-opentofu-goldens.sh");
    assert!(
        fixture_check.contains("generate --output-dir"),
        "{fixture_check}"
    );
    assert!(fixture_check.contains("hostile-config"), "{fixture_check}");
    assert!(
        fixture_check.contains("unknown_config_field"),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains(r#"capture_release_case "$case" "$repo""#),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains(r#"stage_candidate_manifest "$repo" "$case""#),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains("write_fixture_consumer_manifest()")
            && fixture_check
                .contains(r#"cp "$ROOT/fixtures/consumer-release-manifest.json" "$manifest""#)
            && fixture_check.contains("write_fixture_consumer_manifest \"$repo\""),
        "positive fixtures install an explicit schema input only: {fixture_check}"
    );
    assert!(
        fixture_check.contains("capture_release_dogfood"),
        "{fixture_check}"
    );
    assert!(
        !fixture_check.contains("write_release_fixture_manifest"),
        "{fixture_check}"
    );
    let qualification_helpers =
        include_str!("../../../scripts/generator-release/qualification-goldens.sh");
    assert!(
        qualification_helpers.contains(r#"stage_candidate_manifest "$repo" dogfood"#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers
            .contains(r#"cmp -s "$CANDIDATE_MANIFEST" "$repo/.velnor/release-manifest.json""#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("staged_sha\" != \"$CANDIDATE_MANIFEST_SHA256\""),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("GITHUB_SHA"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("host_target"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("candidate manifest digest does not match candidate CLI"),
        "{qualification_helpers}"
    );
}
