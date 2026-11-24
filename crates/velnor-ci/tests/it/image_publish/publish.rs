// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;

use serde_json::{Value, json};

use super::fixture::Candidate;
use super::registry::{Fake, Flaws, REGISTRY_TOKEN, TOKEN};

pub(super) const PASS: &str = "PASS: Publication completed for the job image with its OCI digest. Package access is `NOT_RUN`.\n";

fn digests(candidate: &Candidate) -> (String, Vec<String>) {
    let oci = &candidate.record["oci"];
    let mut blobs: Vec<String> = [oci["image_configuration_digest"].clone()]
        .into_iter()
        .chain(oci["layer_digests"].as_array().cloned().unwrap_or_default())
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();
    blobs.sort();
    blobs.dedup();
    let manifest = oci["platform_manifest_digest"].as_str();
    (manifest.unwrap_or_default().to_owned(), blobs)
}

pub(super) fn refused(candidate: &Candidate, fake: &Fake) -> String {
    let (result, _) = candidate.publish(fake, candidate.env());
    let message = result.err().unwrap_or_else(|| "ACCEPTED".to_owned());
    assert!(!message.contains(TOKEN), "{message}");
    message
}

#[test]
fn publishes_the_platform_manifest_and_every_blob_by_digest_and_writes_the_evidence() {
    let candidate = Candidate::new("publishes");
    let fake = Fake::default();
    let (result, pauses) = candidate.publish(&fake, candidate.env());
    assert_eq!(result.as_deref(), Ok(PASS));
    assert_eq!(pauses, Vec::<std::time::Duration>::new());
    let (manifest, blobs) = digests(&candidate);
    assert_eq!(fake.blobs(), blobs);
    assert_eq!(fake.manifests(), std::slice::from_ref(&manifest));
    for seen in fake.seen() {
        let object = seen.url.contains("/blobs/") || seen.url.contains("/manifests/");
        let addressed = seen.url.contains("sha256:") || seen.url.contains("sha256%3A");
        let uploading = seen.url.contains("/blobs/uploads/");
        assert!(
            !object || addressed || uploading,
            "{} {}",
            seen.method,
            seen.url
        );
        assert!(
            !seen.url.contains("latest") && !seen.url.contains("/tags/"),
            "{}",
            seen.url
        );
    }
    let path = candidate.path("evidence/publication.json");
    let written = fs::read_to_string(&path).unwrap_or_default();
    let evidence: Value = serde_json::from_str(&written).unwrap_or_default();
    assert_eq!(evidence["schema"], "velnor.job-image-publication.v1");
    assert_eq!(evidence["status"], "PASS");
    assert_eq!(
        evidence["digest_reference"],
        format!("ghcr.io/tailrocks/velnor-job@{manifest}")
    );
    assert_eq!(evidence["ci"], json!({"run_id": 50, "run_attempt": 1}));
    assert_eq!(evidence["source"], candidate.head);
    assert!(written.ends_with("}\n") && !written.contains(TOKEN));
    let mode = fs::metadata(&path).map(|meta| meta.permissions().mode() & 0o777);
    assert_eq!(mode.ok(), Some(0o600));
    let calls = candidate.gh_log();
    assert_eq!(calls.matches("attestation verify").count(), 2, "{calls}");
    assert_eq!(calls.matches("commits/main").count(), 3, "{calls}");
}

#[test]
fn the_registry_token_goes_only_to_the_registry_and_the_basic_secret_only_to_the_exchange() {
    let candidate = Candidate::new("credentials");
    let fake = Fake::with(Flaws {
        redirect: Some(
            "https://pkg-containers.githubusercontent.com/ghcr1/blobs/{digest}?se=1".to_owned(),
        ),
        ..Flaws::default()
    });
    let (result, _) = candidate.publish(&fake, candidate.env());
    assert_eq!(result.as_deref(), Ok(PASS));
    let bearer = format!("Bearer {REGISTRY_TOKEN}");
    let mut downloads = 0;
    for seen in fake.seen() {
        if seen
            .url
            .starts_with("https://pkg-containers.githubusercontent.com/")
        {
            downloads += 1;
            assert_eq!(seen.authorization, None, "{}", seen.url);
        } else if seen.url.contains("/token?") {
            assert!(
                seen.authorization
                    .is_some_and(|value| value.starts_with("Basic "))
            );
        } else if seen.url != "https://ghcr.io/v2/" {
            assert_eq!(
                seen.authorization.as_deref(),
                Some(bearer.as_str()),
                "{}",
                seen.url
            );
        }
    }
    assert!(
        downloads >= 3,
        "the blobs were re-read through the redirect"
    );
}

#[test]
fn a_candidate_whose_attestation_check_fails_pushes_nothing() {
    let candidate = Candidate::new("attestation-fails");
    candidate.fail_attestation();
    let fake = Fake::default();
    assert_eq!(refused(&candidate, &fake), "GitHub CLI request failed.");
    assert!(fake.seen().is_empty());
    assert!(!candidate.path("evidence").exists());
    assert!(candidate.gh_log().contains("attestation verify"));
}

#[test]
fn an_attestation_for_another_commit_or_trigger_pushes_nothing() {
    let candidate = Candidate::new("attestation-wrong");
    for (field, value) in [
        ("githubWorkflowSHA", json!("2".repeat(40))),
        ("githubWorkflowTrigger", json!("pull_request")),
        ("sourceRepositoryRef", json!("refs/pull/1/merge")),
        ("sourceRepositoryIdentifier", json!("5")),
    ] {
        let mut cert = json!([{"verificationResult": {"signature": {"certificate": {
            "sourceRepositoryIdentifier": "1390620900",
            "githubWorkflowRepository": "tailrocks/velnor-new",
            "githubWorkflowSHA": candidate.head, "githubWorkflowRef": "refs/heads/main",
            "githubWorkflowTrigger": "workflow_dispatch",
            "sourceRepositoryRef": "refs/heads/main",
            "sourceRepositoryDigest": candidate.head}}}}]);
        cert[0]["verificationResult"]["signature"]["certificate"][field] = value;
        fs::write(candidate.path("attestation.json"), cert.to_string())
            .unwrap_or_else(|e| unreachable!("{e}"));
        let fake = Fake::default();
        let message = refused(&candidate, &fake);
        assert!(
            message.starts_with("Attestation certificate"),
            "{field}: {message}"
        );
        assert!(fake.seen().is_empty(), "{field}");
    }
}

#[test]
fn a_pull_request_or_foreign_ref_is_refused_before_any_request_or_gh_call() {
    let candidate = Candidate::new("refused-ref");
    for (name, value) in [
        ("GITHUB_REF", "refs/pull/7/merge"),
        ("GITHUB_REF", "refs/tags/v1"),
        ("GITHUB_REF", "refs/heads/feature"),
        ("GITHUB_EVENT_NAME", "pull_request"),
        ("GITHUB_EVENT_NAME", "push"),
    ] {
        let fake = Fake::default();
        let mut vars = candidate.env();
        vars.insert(name.to_owned(), value.to_owned());
        let (result, _) = candidate.publish(&fake, vars);
        assert_eq!(
            result.err().as_deref(),
            Some("Publication is only for workflow_dispatch on refs/heads/main."),
            "{name}={value}"
        );
        assert!(fake.seen().is_empty());
        assert_eq!(candidate.gh_log(), "", "{name}={value}: gh was not called");
        assert!(!candidate.path("evidence").exists());
    }
}

#[test]
fn a_missing_credential_or_artifact_id_is_refused_before_any_request() {
    let candidate = Candidate::new("missing");
    for name in ["GITHUB_ACTOR", "VELNOR_CANDIDATE_ARTIFACT_ID", "GITHUB_SHA"] {
        let fake = Fake::default();
        let mut vars = candidate.env();
        vars.remove(name);
        let (result, _) = candidate.publish(&fake, vars);
        assert_eq!(
            result.err().as_deref(),
            Some("Required GitHub workflow identity is missing."),
            "{name}"
        );
        assert!(fake.seen().is_empty());
        assert_eq!(candidate.gh_log(), "");
    }
}
