use super::*;

#[test]
fn manual_recovery_has_no_build_or_signing_authority() {
    let rendered = render_preview(&fixture()).expect("preview");
    let yaml = &rendered.bytes;
    assert!(yaml.contains("original_run_id:"));
    assert!(yaml.contains("original_run_attempt:"));
    assert!(yaml.contains("description: Original preview production workflow run ID"));
    assert!(yaml.contains("description: Original preview production workflow run attempt"));
    assert!(!yaml.contains("Original push workflow run ID"));
    assert!(!yaml.contains("Original signer run attempt"));
    for (name, next) in [
        ("preview-payload", "preview-assemble"),
        ("preview-assemble", "preview-signer"),
        ("preview-signer", "preview-publisher"),
        ("preview-publisher", "preview-recovery"),
    ] {
        assert!(job(yaml, name, Some(next)).contains("github.event_name == 'workflow_run'"));
    }
    let recovery = job(yaml, "preview-recovery", None);
    assert!(recovery.contains("github.event_name == 'workflow_dispatch'"));
    assert!(recovery.contains("recover-package-release-v1"));
    assert!(!recovery.contains("actions/checkout"));
    assert!(!recovery.contains("execute-release-producer-v1"));
    assert!(!recovery.contains("sign-package-release-v1"));
    assert!(!recovery.contains("id-token"));
    assert!(!recovery.contains("attestations: write"));
    assert!(recovery.contains("${{ secrets.RELEASE_STATUS_READ_TOKEN }}"));
}
