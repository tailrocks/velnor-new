use super::super::{
    DIR, FILE, candidate_path, manifest_attestation_bundle_script, manifest_script,
};
use super::MANIFEST_PRODUCER;
use velnor_actions_contract_release::RELEASE_MANIFEST_FILENAME;

#[test]
fn manifest_bundle_uses_the_created_and_attested_file() {
    let create = manifest_script("1.98.1", "1.21.1");
    let fetch = manifest_attestation_bundle_script();
    assert!(create.contains("create-release-manifest.sh"));
    assert!(
        fetch.contains("subject='manifest-assets/release-manifest.json'"),
        "{fetch}"
    );
    assert!(!fetch.contains(&format!("subject='{FILE}'")));
}

#[test]
fn renderer_uses_the_public_canonical_manifest_filename() {
    assert_eq!(FILE, RELEASE_MANIFEST_FILENAME);
    assert_eq!(
        candidate_path(),
        format!("{DIR}/{RELEASE_MANIFEST_FILENAME}")
    );
    assert!(
        MANIFEST_PRODUCER.contains(&format!("> {RELEASE_MANIFEST_FILENAME}")),
        "the shell producer must write the public canonical filename"
    );
}
