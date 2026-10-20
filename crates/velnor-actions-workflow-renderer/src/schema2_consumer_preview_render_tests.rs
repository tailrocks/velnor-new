use super::*;

#[test]
fn preview_has_isolated_privileges_and_exact_inventory_attestation() {
    let rendered = render_consumer_preview(&fixture(), &hosted_policy()).expect("preview");
    assert_eq!(rendered.path, ".github/workflows/binary-preview.yml");
    let yaml = &rendered.bytes;
    let triggers = &yaml[..yaml.find("jobs:").expect("jobs")];
    assert!(triggers.contains("workflow_run:"));
    assert!(triggers.contains("workflows:"));
    assert!(triggers.contains("- CI"));
    assert!(triggers.contains("- completed"));
    assert!(!triggers.contains("  push:"));
    let eligibility = job(yaml, "preview-eligibility", Some("preview-payload-000"));
    assert!(eligibility.contains("ref: ${{ github.sha }}"));
    assert_eq!(eligibility.matches("actions/checkout").count(), 1);
    assert!(eligibility.find("actions/checkout") < eligibility.find("Verify source and CI"));
    assert!(eligibility.contains("fetch-depth: 0"));
    assert!(eligibility.contains("persist-credentials: false"));
    let producer = job(yaml, "preview-payload-000", Some("preview-payload-001"));
    assert!(producer.contains("fetch-depth: 0"));
    assert!(producer.contains("persist-credentials: false"));
    assert!(!producer.contains("environment:"));
    assert!(!producer.contains("GH_TOKEN: \"${{ github.token }}\""));
    assert!(!producer.contains("id-token"));
    assert!(!producer.contains("contents: write"));
    assert!(!producer.contains("RELEASE_STATUS_READ_TOKEN"));
    let signer = job(yaml, "preview-signer", Some("preview-publisher"));
    assert!(!signer.contains("actions/checkout"));
    assert!(signer.contains("id-token: write"));
    assert!(signer.contains("contents: read"));
    assert!(!signer.contains("contents: write"));
    assert_eq!(signer.matches("/velnor/release-signed/").count(), 8);
    assert!(!signer.contains("artifact-metadata"));
    assert!(signer.contains("create-storage-record: false"));
    assert!(!signer.contains("RELEASE_STATUS_READ_TOKEN"));
    let publisher = job(yaml, "preview-publisher", None);
    assert!(!publisher.contains("actions/checkout"));
    assert!(!publisher.contains("id-token"));
    assert!(publisher.contains("contents: write"));
    assert!(publisher.contains("publish-package-release-v1"));
    assert!(publisher.contains("${{ secrets.RELEASE_STATUS_READ_TOKEN }}"));
    assert!(!yaml.contains("tap-update"));
}

#[test]
fn preview_rejects_stable_plan_or_invalid_inventory_before_rendering() {
    let mut spec = fixture();
    spec.policy.channel = PackageReleaseChannel::Stable;
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
    spec.policy.channel = PackageReleaseChannel::Preview;
    spec.policy.default_branch = "develop".to_owned();
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
    spec.policy.default_branch = "main".to_owned();
    spec.policy.products[0]
        .targets
        .push("../../escape".to_owned());
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
}

#[test]
fn manual_recovery_has_no_build_or_signing_authority() {
    let rendered = render_consumer_preview(&fixture(), &hosted_policy()).expect("preview");
    let yaml = &rendered.bytes;
    assert!(yaml.contains("original_run_id:"));
    assert!(yaml.contains("original_run_attempt:"));
    assert!(yaml.contains("description: Original preview production workflow run ID"));
    assert!(yaml.contains("description: Original preview production workflow run attempt"));
    assert!(!yaml.contains("Original push workflow run ID"));
    assert!(!yaml.contains("Original signer run attempt"));
    for (name, next) in [
        ("preview-payload-000", "preview-payload-001"),
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
