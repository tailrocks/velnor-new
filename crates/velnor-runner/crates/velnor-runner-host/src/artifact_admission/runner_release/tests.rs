// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::*;

const SIGNER: &str = "product-release-images.yml@refs/heads/main";
const SOURCE: &str = "1111111111111111111111111111111111111111";
const SUMS_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MANIFEST_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OTHER_DIGEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

struct ModelHelper;

impl SubjectVerifier for ModelHelper {
    async fn verify(
        &self,
        bundle: &Value,
        subject: SubjectRef<'_>,
        peer_subject: SubjectRef<'_>,
    ) -> Result<(), HostError> {
        tokio::task::yield_now().await;
        let subjects = bundle["subjects"].as_array().ok_or(HostError::Identity)?;
        let digest_of = |name: &str| {
            let named: Vec<_> = subjects.iter().filter(|s| s["name"] == name).collect();
            (named.len() <= 1).then(|| named.first().map(|s| s["digest"].as_str()))
        };
        let claims_match = bundle["signer"] == SIGNER && bundle["source"] == SOURCE;
        let subject_match = digest_of(subject.name) == Some(Some(Some(subject.digest)));
        let peer_match = match digest_of(peer_subject.name) {
            Some(None) => true,
            Some(Some(digest)) => digest == Some(peer_subject.digest),
            None => false,
        };
        if claims_match && subject_match && peer_match {
            Ok(())
        } else {
            Err(HostError::Identity)
        }
    }
}

fn bundle(run: u64, subjects: &[(&str, &str)]) -> Value {
    json!({
        "signer": SIGNER,
        "source": SOURCE,
        "run": run,
        "subjects": subjects
            .iter()
            .map(|(name, digest)| json!({"name": name, "digest": digest}))
            .collect::<Vec<_>>(),
    })
}

fn sums_only(run: u64) -> Value {
    bundle(run, &[("SHA256SUMS", SUMS_DIGEST)])
}

fn manifest_only(run: u64) -> Value {
    bundle(run, &[("RUNNER_IMAGE_MANIFEST.json", MANIFEST_DIGEST)])
}

fn both(run: u64) -> Value {
    bundle(
        run,
        &[
            ("SHA256SUMS", SUMS_DIGEST),
            ("RUNNER_IMAGE_MANIFEST.json", MANIFEST_DIGEST),
        ],
    )
}

async fn admit(checksum: &[Value], manifest: &[Value]) -> Result<(), HostError> {
    admit_attestations(
        &ModelHelper,
        checksum,
        manifest,
        SubjectRef {
            name: "SHA256SUMS",
            digest: SUMS_DIGEST,
        },
        SubjectRef {
            name: "RUNNER_IMAGE_MANIFEST.json",
            digest: MANIFEST_DIGEST,
        },
    )
    .await
}

fn identity_error(result: Result<(), HostError>) -> bool {
    matches!(result, Err(HostError::Identity))
}

#[tokio::test]
async fn admits_separate_per_file_statements() {
    assert!(admit(&[sums_only(1)], &[manifest_only(1)]).await.is_ok());
}

#[tokio::test]
async fn admits_one_statement_naming_both_files() {
    assert!(admit(&[both(1)], &[both(1)]).await.is_ok());
}

#[tokio::test]
async fn admits_a_rerun_that_leaves_a_second_valid_statement() {
    assert!(
        admit(&[both(1), both(2)], &[both(1), both(2)])
            .await
            .is_ok()
    );
    assert!(
        admit(
            &[sums_only(1), sums_only(2)],
            &[manifest_only(1), manifest_only(2)]
        )
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn rejects_a_release_without_both_attestations() {
    assert!(identity_error(admit(&[], &[manifest_only(1)]).await));
    assert!(identity_error(admit(&[sums_only(1)], &[]).await));
}

#[tokio::test]
async fn rejects_a_checksum_subject_for_other_bytes() {
    let other = bundle(1, &[("SHA256SUMS", OTHER_DIGEST)]);
    assert!(identity_error(admit(&[other], &[manifest_only(1)]).await));
}

#[tokio::test]
async fn rejects_a_second_statement_for_another_signer_or_source() {
    let mut signer = both(2);
    signer["signer"] = json!("other.yml@refs/heads/main");
    let mut source = both(2);
    source["source"] = json!("2222222222222222222222222222222222222222");
    for second in [signer, source] {
        assert!(identity_error(
            admit(&[both(1), second.clone()], &[both(1)]).await
        ));
        assert!(identity_error(admit(&[both(1)], &[both(1), second]).await));
    }
}

#[tokio::test]
async fn rejects_a_second_statement_for_another_manifest() {
    let other = bundle(
        2,
        &[
            ("SHA256SUMS", SUMS_DIGEST),
            ("RUNNER_IMAGE_MANIFEST.json", OTHER_DIGEST),
        ],
    );
    assert!(identity_error(admit(&[both(1), other], &[both(1)]).await));
    let other_manifest = bundle(2, &[("RUNNER_IMAGE_MANIFEST.json", OTHER_DIGEST)]);
    assert!(identity_error(
        admit(&[both(1)], &[both(1), other_manifest]).await
    ));
}
