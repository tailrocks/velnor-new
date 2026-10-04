use super::action_snapshots::{Actions, action};

pub(super) fn assert_candidate_qualification(
    actions: &Actions,
    action_name: &str,
    build_job: &str,
    directory: &str,
    binary: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let job = action(actions, action_name)?;
    let archive_extract = job
        .find("Prevalidate and extract uploaded candidate archive")
        .ok_or("missing prevalidated candidate archive extraction")?;
    let file_modes = job
        .find("Verify extracted candidate file types and executable mode")
        .ok_or("missing extracted candidate file-type check")?;
    let checksum = job
        .find("Verify downloaded checksum sidecar")
        .ok_or("missing downloaded checksum verification")?;
    let provenance = job
        .find("Verify candidate provenance record")
        .ok_or("missing candidate provenance verification")?;
    let qualification = job
        .find("Qualify downloaded candidate")
        .ok_or("missing exact candidate qualification")?;
    let artifact_id = format!("artifact-ids: ${{{{ needs.{build_job}.outputs.artifact_id }}}}");
    assert!(
        archive_extract < file_modes
            && file_modes < provenance
            && provenance < checksum
            && checksum < qualification,
        "{job}"
    );
    assert!(job.contains(&artifact_id), "{job}");
    assert!(!job.contains("Attest built artifacts"), "{job}");
    assert!(!job.contains("GH_TOKEN:"), "{job}");
    assert!(!job.contains("actions/upload-artifact@"), "{job}");
    assert_eq!(job.matches("Qualify downloaded candidate").count(), 1);
    assert!(job.contains("test -x "), "{job}");
    assert!(job.contains("toolchain"), "{job}");
    assert!(job.contains("--version"), "{job}");
    assert!(
        job.contains(&format!(
            "scripts/capture-opentofu-goldens.sh check-release \\\"$GITHUB_WORKSPACE/{directory}/{binary}\\\""
        )),
        "{job}"
    );
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
        fixture_check.contains(r#"version=$("$BIN" --version"#),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains(r#"capture_release_case "$case" "$repo""#),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains(r#"write_release_fixture_manifest "$repo""#),
        "{fixture_check}"
    );
    Ok(())
}
