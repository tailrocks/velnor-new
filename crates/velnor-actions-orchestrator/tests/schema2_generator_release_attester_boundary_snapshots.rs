use std::error::Error;

pub(super) fn assert_attester_boundary(
    workflow: &str,
    attest: &str,
    build: &str,
    qualify: &str,
) -> Result<(), Box<dyn Error>> {
    let attester = super::super::job_body(workflow, attest)?;
    assert_needs_and_artifact(attester, build, qualify);
    assert_attestation_permissions(attester);
    assert_attestation_steps(attester);
    assert_attester_does_not_qualify(attester, qualify);
    Ok(())
}

fn assert_needs_and_artifact(attester: &str, build: &str, qualify: &str) {
    assert!(attester.contains(&format!("- {build}\n")), "{attester}");
    assert!(attester.contains(&format!("- {qualify}\n")), "{attester}");
    assert!(attester.contains("- candidate-manifest\n"), "{attester}");
    assert!(
        attester.contains(&format!(
            "artifact-ids: ${{{{ needs.{build}.outputs.artifact_id }}}}"
        )),
        "{attester}"
    );
    assert!(attester.contains("artifact-ids: ${{ needs."), "{attester}");
}

fn assert_attestation_permissions(attester: &str) {
    assert!(attester.contains("id-token: write"), "{attester}");
    assert!(!attester.contains("contents: write"), "{attester}");
}

fn assert_attestation_steps(attester: &str) {
    for step in [
        "Verify candidate provenance record",
        "Verify downloaded checksum sidecar",
        "Attest built artifacts",
        "Fetch and verify candidate attestation bundles",
        "Upload verified candidate attestation bundles",
    ] {
        assert!(attester.contains(step), "missing {step}: {attester}");
    }
}

fn assert_attester_does_not_qualify(attester: &str, qualify: &str) {
    assert!(
        !attester.contains("Qualify downloaded candidate"),
        "{attester}"
    );
    assert!(!attester.contains("--version"), "{attester}");
    assert!(
        !attester.contains("capture-opentofu-goldens.sh"),
        "{attester}"
    );
    assert!(
        !attester.contains(&format!("needs.{qualify}.outputs")),
        "{attester}"
    );
}
