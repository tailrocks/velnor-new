//! Strict tests for the pinned `gh attestation verify --format json` boundary.

use super::*;
use serde_json::{Value, json};

fn expectations() -> AttestationExpectation<'static> {
    AttestationExpectation {
        repository_uri: "https://github.com/jackin-project/jackin",
        owner_uri: "https://github.com/jackin-project",
        workflow_uri: "https://github.com/jackin-project/jackin/.github/workflows/binary-preview.yml@refs/heads/main",
        workflow_sha: "b12e73afd75b944a872e8295d61a15f93f5789a2",
        source_ref: "refs/heads/main",
        run_id: 200,
        run_attempt: 2,
        repository_id: "123456".to_owned(),
        trigger: "workflow_run",
        invocation_uri: "https://github.com/jackin-project/jackin/actions/runs/200/attempts/2",
    }
}

fn subjects() -> BTreeMap<String, String> {
    [
        ("jackin-aarch64-apple-darwin.tar.gz", "a"),
        ("jackin-aarch64-unknown-linux-gnu.tar.gz", "b"),
        ("jackin-x86_64-apple-darwin.tar.gz", "c"),
        ("jackin-x86_64-pc-windows-msvc.tar.gz", "d"),
        ("jackin-x86_64-unknown-linux-gnu.tar.gz", "e"),
        ("jackin-x86_64-unknown-linux-musl.tar.gz", "f"),
        ("capsule-manifest.json", "1"),
        ("release.json", "2"),
    ]
    .into_iter()
    .map(|(name, digit)| (name.to_owned(), digit.repeat(64)))
    .collect()
}

fn output() -> Value {
    let expected = expectations();
    let subjects = subjects()
        .into_iter()
        .map(|(name, digest)| json!({"name":name,"digest":{"sha256":digest}}))
        .collect::<Vec<_>>();
    json!([{
        "attestation": {},
        "verificationResult": {
            "statement": {"predicateType":PREDICATE,"subject":subjects,"predicate":{"buildDefinition":{"internalParameters":{"github":{"event_name":expected.trigger}}}}},
            "signature": {"certificate": {
                "issuer": OIDC_ISSUER,
                "subjectAlternativeName": expected.workflow_uri,
                "sourceRepositoryURI": expected.repository_uri,
                "sourceRepositoryOwnerURI": expected.owner_uri,
                "sourceRepositoryIdentifier": expected.repository_id,
                "sourceRepositoryDigest": expected.workflow_sha,
                "sourceRepositoryRef": expected.source_ref,
                "buildSignerURI": expected.workflow_uri,
                "buildSignerDigest": expected.workflow_sha,
                "buildConfigURI": expected.workflow_uri,
                "buildConfigDigest": expected.workflow_sha,
                "buildTrigger": expected.trigger,
                "runInvocationURI": expected.invocation_uri,
                "runnerEnvironment": "github-hosted"
            }}
        }
    }])
}

fn validate(value: &Value) -> Result<(), OrchestratorError> {
    let subjects = subjects();
    validate_output(
        value.to_string().as_bytes(),
        &subjects,
        &expectations(),
        "jackin-aarch64-apple-darwin.tar.gz",
        &"a".repeat(64),
    )
}

#[test]
fn accepts_exact_eight_subjects_and_certificate_identity_claims() {
    let value = output();
    serde_json::from_value::<Vec<VerifiedAttestation>>(value.clone())
        .expect("pinned CLI json model");
    validate(&value).expect("pinned CLI result should match exact run identity");
}

#[test]
fn parses_a_real_strictly_verified_github_cli_result() {
    // Captured with `gh 2.102.0 attestation verify release-manifest.json
    // --repo tailrocks/velnor-new --source-ref refs/heads/main --source-digest
    // f1b044a606b6bc469f9718644f5c0ed88bf965b6 --signer-workflow
    // tailrocks/velnor-new/.github/workflows/product-release-generator.yml
    // --signer-digest f1b044a606b6bc469f9718644f5c0ed88bf965b6
    // --predicate-type https://slsa.dev/provenance/v1 --deny-self-hosted-runners
    // --limit 10 --format json`, using an explicit read-only GH_TOKEN.
    // The fixture retains only the CLI's verified result, not its signature bundle.
    let bytes = include_str!("../tests/fixtures/gh-attestation-velnor-0.1.12.json");
    let attestations: Vec<VerifiedAttestation> =
        serde_json::from_str(bytes).expect("real pinned CLI result schema");
    assert_eq!(attestations.len(), 1);
    let verified = &attestations[0].verification_result;
    assert_eq!(verified.statement.predicate_type, PREDICATE);
    assert_eq!(verified.statement.subject.len(), 1);
    let certificate = &verified.signature.certificate;
    assert_eq!(certificate.issuer, OIDC_ISSUER);
    assert_eq!(
        certificate.build_signer_uri,
        "https://github.com/tailrocks/velnor-new/.github/workflows/product-release-generator.yml@refs/heads/main"
    );
    assert_eq!(
        certificate.build_config_uri,
        "https://github.com/tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main"
    );
    assert_eq!(certificate.source_repository_identifier, "1390620900");
}

#[test]
fn signer_workflow_argument_pins_host_repository_path_and_ref() {
    let identity = PackageReleaseSigningIdentity {
        repository: "jackin-project/jackin".to_owned(),
        repository_id: 123_456,
        source_sha: "a12e73afd75b944a872e8295d61a15f93f5789a2".to_owned(),
        source_ref: "refs/heads/main".to_owned(),
        default_branch: "main".to_owned(),
        event: velnor_actions_contract::package_release::PackageReleaseEvent::WorkflowRun,
        workflow_path: ".github/workflows/binary-preview.yml".to_owned(),
        workflow_ref: "jackin-project/jackin/.github/workflows/binary-preview.yml@refs/heads/main"
            .to_owned(),
        workflow_sha: "b12e73afd75b944a872e8295d61a15f93f5789a2".to_owned(),
        run_id: 200,
        run_attempt: 2,
        ci_run_id: 199,
        ci_run_attempt: 1,
        commit_count: Some(7),
    };
    assert_eq!(
        signer_workflow_argument(&identity),
        "github.com/jackin-project/jackin/.github/workflows/binary-preview.yml@refs/heads/main"
    );
}

#[test]
fn source_attestation_token_is_required_and_disjoint_from_tap_writer_token() {
    assert_eq!(
        validate_source_attestation_token(Some("source-read".to_owned()), None)
            .expect("source token"),
        "source-read"
    );
    assert!(validate_source_attestation_token(None, None).is_err());
    assert!(validate_source_attestation_token(Some(String::new()), None).is_err());
    assert!(
        validate_source_attestation_token(
            Some("same-token".to_owned()),
            Some("same-token".to_owned())
        )
        .is_err()
    );
}

#[test]
fn rejects_wrong_source_workflow_issuer_or_run_claims() {
    let cases = vec![
        ("issuer", "https://wrong.example".to_owned()),
        ("sourceRepositoryDigest", "b".repeat(40)),
        ("sourceRepositoryRef", "refs/heads/other".to_owned()),
        ("sourceRepositoryIdentifier", "123457".to_owned()),
        (
            "buildSignerURI",
            "https://github.com/other/workflow.yml@refs/heads/main".to_owned(),
        ),
        ("buildConfigDigest", "b".repeat(40)),
        ("buildTrigger", "workflow_dispatch".to_owned()),
        ("buildTrigger", "push".to_owned()),
        (
            "runInvocationURI",
            "https://github.com/jackin-project/jackin/actions/runs/199/attempts/2".to_owned(),
        ),
        ("runnerEnvironment", "self-hosted".to_owned()),
    ];
    for (field, value) in cases {
        let mut data = output();
        data[0]["verificationResult"]["signature"]["certificate"][field] = Value::String(value);
        assert!(validate(&data).is_err(), "must reject {field}");
    }
}

#[test]
fn slsa_event_binds_actual_channel_and_rejects_old_push_or_missing_event() {
    assert!(validate(&output()).is_ok());
    let pointer = "/0/verificationResult/statement/predicate/buildDefinition/internalParameters/github/event_name";
    for event in [
        json!("push"),
        json!("pull_request"),
        json!("workflow_dispatch"),
        Value::Null,
    ] {
        let mut value = output();
        *value.pointer_mut(pointer).expect("SLSA event") = event;
        assert!(validate(&value).is_err());
    }
    let mut missing = output();
    missing[0]["verificationResult"]["statement"]["predicate"]["buildDefinition"]["internalParameters"]
        ["github"] = json!({});
    assert!(validate(&missing).is_err());
    let mut stable = output();
    *stable.pointer_mut(pointer).expect("stable SLSA event") = json!("workflow_dispatch");
    stable[0]["verificationResult"]["signature"]["certificate"]["buildTrigger"] =
        json!("workflow_dispatch");
    let subjects = subjects();
    let mut expected = expectations();
    expected.trigger = "workflow_dispatch";
    validate_output(
        stable.to_string().as_bytes(),
        &subjects,
        &expected,
        "jackin-aarch64-apple-darwin.tar.gz",
        &"a".repeat(64),
    )
    .expect("matching stable certificate and SLSA event");
}

#[test]
fn rejects_missing_extra_duplicate_and_misdigested_attestation_subjects() {
    let mut missing = output();
    missing[0]["verificationResult"]["statement"]["subject"]
        .as_array_mut()
        .expect("subject array")
        .pop();
    assert!(validate(&missing).is_err());

    let mut extra = output();
    extra[0]["verificationResult"]["statement"]["subject"]
        .as_array_mut()
        .expect("subject array")
        .push(json!({"name":"extra.tar.gz","digest":{"sha256":"a".repeat(64)}}));
    assert!(validate(&extra).is_err());

    let mut wrong = output();
    wrong[0]["verificationResult"]["statement"]["subject"][0]["digest"]["sha256"] =
        Value::String("f".repeat(64));
    assert!(validate(&wrong).is_err());

    let mut duplicate = output();
    let first = duplicate[0]["verificationResult"]["statement"]["subject"][0].clone();
    duplicate[0]["verificationResult"]["statement"]["subject"]
        .as_array_mut()
        .expect("subject array")
        .push(first);
    assert!(validate(&duplicate).is_err());

    let mut multiple = output();
    multiple
        .as_array_mut()
        .expect("result list")
        .push(json!({}));
    assert!(validate(&multiple).is_err());
}

#[test]
fn rejects_wrong_predicate_and_malformed_json() {
    let mut wrong = output();
    wrong[0]["verificationResult"]["statement"]["predicateType"] =
        Value::String("https://example.invalid/predicate".to_owned());
    assert!(validate(&wrong).is_err());
    assert!(
        validate_output(
            b"not json",
            &subjects(),
            &expectations(),
            "jackin-aarch64-apple-darwin.tar.gz",
            &"a".repeat(64),
        )
        .is_err()
    );
}
