use serde_json::{Value, json};

use super::super::super::{ProviderFailure, SOURCE_REPOSITORY};
use super::verify_asset_attestation;

const SOURCE_REVISION: &str = "f1b044a606b6bc469f9718644f5c0ed88bf965b6";
const SIGNER_IDENTITY: &str = "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main";
const PROBE_ARCHIVE: &str = "velnor-resource-probe-linux-amd64.tar";
const PROBE_DIGEST: &str = "14a1ef2eed06ea2a55047277104f5f3a5d651bbd2425b60c549fed02eb3de714";

fn subject(name: &str, digest: &str) -> Value {
    json!({"name": name, "digest": {"sha256": digest}})
}

fn actual_release_subjects() -> Vec<Value> {
    vec![
        subject(
            "velnor-runner-linux-amd64.tar",
            "e977f106c83c6c859a8b0899e7a57863f65144001bf9847a23a1022dbfb2dfd4",
        ),
        subject(
            "velnor-dind-linux-amd64.tar",
            "d81f37edff507694a90a07d36cb87115ffdfa8160f266687e506229dfcc19f6d",
        ),
        subject(PROBE_ARCHIVE, PROBE_DIGEST),
        subject(
            "RESOURCE_PROBE_MANIFEST.json",
            "9312a54f13471624851c7a1cfe4e975560a5c4a12859a662c98125914f062ae9",
        ),
        subject(
            "SHA256SUMS",
            "b83d26f58f1e7dfd73165b8ed7f7b49b5f4105a95e1876a1384f984a4f1861b7",
        ),
    ]
}

fn attestation(subjects: &[Value]) -> Value {
    json!([{
        "verificationResult": {
            "signature": {
                "certificate": {
                    "subjectAlternativeName": SIGNER_IDENTITY,
                    "issuer": "https://token.actions.githubusercontent.com",
                    "sourceRepositoryURI": "https://github.com/tailrocks/velnor-new",
                    "sourceRepositoryDigest": SOURCE_REVISION,
                    "sourceRepositoryRef": "refs/heads/main",
                    "buildSignerURI": SIGNER_IDENTITY,
                    "buildSignerDigest": SOURCE_REVISION,
                    "githubWorkflowRepository": SOURCE_REPOSITORY,
                    "githubWorkflowRef": "refs/heads/main",
                    "githubWorkflowSHA": SOURCE_REVISION,
                }
            },
            "statement": {
                "predicateType": "https://slsa.dev/provenance/v1",
                "subject": subjects,
            }
        }
    }])
}

#[test]
fn actual_release_attestation_accepts_target_among_five_subjects() {
    assert_eq!(
        verify_asset_attestation(
            &attestation(&actual_release_subjects()),
            PROBE_ARCHIVE,
            PROBE_DIGEST,
            SOURCE_REVISION,
        ),
        Ok(())
    );
}

#[test]
fn duplicate_target_subject_is_rejected() {
    let mut subjects = actual_release_subjects();
    subjects.push(subject(PROBE_ARCHIVE, PROBE_DIGEST));
    assert_eq!(
        verify_asset_attestation(
            &attestation(&subjects),
            PROBE_ARCHIVE,
            PROBE_DIGEST,
            SOURCE_REVISION,
        ),
        Err(ProviderFailure::Attestation)
    );
}

#[test]
fn duplicate_unrelated_subject_is_rejected() {
    let mut subjects = actual_release_subjects();
    subjects.push(subject(
        "velnor-runner-linux-amd64.tar",
        "e977f106c83c6c859a8b0899e7a57863f65144001bf9847a23a1022dbfb2dfd4",
    ));
    assert_eq!(
        verify_asset_attestation(
            &attestation(&subjects),
            PROBE_ARCHIVE,
            PROBE_DIGEST,
            SOURCE_REVISION,
        ),
        Err(ProviderFailure::Attestation)
    );
}

#[test]
fn mismatched_target_digest_is_rejected() {
    assert_eq!(
        verify_asset_attestation(
            &attestation(&actual_release_subjects()),
            PROBE_ARCHIVE,
            &"0".repeat(64),
            SOURCE_REVISION,
        ),
        Err(ProviderFailure::Attestation)
    );
}
