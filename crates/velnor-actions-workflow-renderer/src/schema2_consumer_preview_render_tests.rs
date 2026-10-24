use super::*;

#[test]
fn preview_has_isolated_privileges_and_exact_inventory_attestation() {
    let spec = fixture();
    let rendered = render_consumer_preview(&spec, &hosted_policy()).expect("preview");
    assert_eq!(rendered.path, ".github/workflows/binary-preview.yml");
    let yaml = &rendered.bytes;
    let triggers = &yaml[..yaml.find("jobs:").expect("jobs")];
    assert!(triggers.contains("workflow_run:"));
    assert!(triggers.contains("workflows:"));
    assert!(triggers.contains("- CI"));
    assert!(triggers.contains("- completed"));
    assert!(!triggers.contains("  push:"));
    let eligibility = job(yaml, "preview-eligibility", Some("preview-payload"));
    let select = eligibility
        .find("select-preview-source-v1")
        .expect("source selector");
    let checkout = eligibility
        .find("actions/checkout@")
        .expect("exact-source checkout");
    let finalize = eligibility
        .find("finalize-preview-source-v1")
        .expect("source finalizer");
    assert!(select < checkout && checkout < finalize);
    assert_eq!(eligibility.matches("actions/checkout@").count(), 1);
    assert!(eligibility.contains("ref: ${{ steps.source-select.outputs.source_sha }}"));
    assert!(eligibility.contains("fetch-depth: 0"));
    assert!(eligibility.contains("persist-credentials: false"));
    assert!(eligibility.contains("fetch-depth: 0"));
    assert!(eligibility.contains("persist-credentials: false"));
    // The admitted producer rows share one job-level permission boundary.
    let producer = job(yaml, "preview-payload", Some("preview-assemble"));
    assert!(
        producer
            .contains("matrix: ${{ fromJSON(needs.preview-eligibility.outputs.payload_matrix) }}")
    );
    assert!(producer.contains("fetch-depth: 0"));
    assert!(producer.contains("persist-credentials: false"));
    assert!(!producer.contains("environment:"));
    assert!(!producer.contains("GH_TOKEN: \"${{ github.token }}\""));
    assert!(!producer.contains("id-token"));
    assert!(!producer.contains("contents: write"));
    let executor = producer
        .find("execute-release-producer-v1")
        .expect("native producer executor");
    assert!(producer[..executor].contains("${{ secrets.RELEASE_STATUS_READ_TOKEN }}"));
    assert!(producer[executor..].contains("unset VELNOR_RELEASE_STATUS_TOKEN"));
    assert!(!producer[executor..].contains("${{ secrets.RELEASE_STATUS_READ_TOKEN }}"));
    let signer_yaml = job(yaml, "preview-signer", Some("preview-publisher"));
    assert!(!signer_yaml.contains("actions/checkout"));
    assert!(signer_yaml.contains("id-token: write"));
    assert!(signer_yaml.contains("contents: read"));
    assert!(!signer_yaml.contains("contents: write"));
    assert!(signer_yaml.contains("admit-release-aggregate-v1"));
    assert!(signer_yaml.contains("steps.release.outputs.attestation_subject_paths"));
    assert!(!signer_yaml.contains("artifact-metadata"));
    assert!(signer_yaml.contains("create-storage-record: false"));
    let (_, signer_document) = signer(&spec).expect("signer job");
    let Yaml::Seq(steps) = field(&signer_document, "steps") else {
        panic!("signer steps");
    };
    assert_job_credentials_are_native_only(&signer_document);
    let attestation_step = steps
        .iter()
        .find(|step| {
            let Yaml::Map(fields) = step else {
                return false;
            };
            fields.iter().any(|(key, value)| {
                key == "uses" && value == &Yaml::str(crate::steps::ATTEST_BUILD_PROVENANCE_USES)
            })
        })
        .expect("pinned provenance step");
    assert_eq!(
        field(field(attestation_step, "with"), "subject-path"),
        &Yaml::str("${{ steps.release.outputs.attestation_subject_paths }}")
    );
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

fn field<'a>(yaml: &'a Yaml, name: &str) -> &'a Yaml {
    let Yaml::Map(fields) = yaml else {
        panic!("{name} map");
    };
    &fields.iter().find(|(key, _)| key == name).expect(name).1
}
