// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

const SOURCE: &str = "a3417b150ac2f477498cadc329cdf4e23aa5b54b";
const WORKFLOW: &str = "https://github.com/tailrocks/velnor-new/.github/workflows/job-image-candidate.yml@refs/heads/main";
const RECORD: &str = "job-image-candidate.json";
const TEST: &str = "package_release_signer::attestation::image_fixture_tests::signed_image_record_binds_source_run_and_subjects";

#[test]
fn signed_image_record_binds_source_run_and_subjects() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("VELNOR_IMAGE_ATTESTATION_CHILD").is_some() {
        super::image_transport_tests::validate(TEST)?;
        let error = std::net::UdpSocket::bind("127.0.0.1:0")
            .expect_err("image attestation child must be network isolated");
        if error.kind() != std::io::ErrorKind::PermissionDenied {
            return Err("image attestation child lacks the network-denial boundary".into());
        }
        return verify_record();
    }
    if std::env::var_os("VELNOR_IMAGE_NONCE_PROBE").is_some() {
        super::image_transport_tests::validate_probe_context(TEST)?;
        assert!(super::image_transport_tests::validate(TEST).is_err());
        return Ok(());
    }
    if std::env::var_os("VELNOR_GH_FIXTURE_WORKER").is_none() {
        return super::image_transport_tests::dispatch(TEST);
    }
    super::image_transport_tests::validate(TEST)?;
    probe_invalid_nonces()?;
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let output = crate::offline_attestation_test::run_validated_worker(
        TEST,
        "VELNOR_IMAGE_ATTESTATION_CHILD",
        &fixtures.join("image-job"),
        &fixtures.join("historical/trusted-root.jsonl"),
    )?;
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
    reject_forged_child(&fixtures)?;
    super::image_transport_tests::record_success(TEST)?;
    Ok(())
}

fn reject_forged_child(fixtures: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = std::process::Command::new("/usr/bin/python3");
    command
        .arg(fixtures.join("no_network.py"))
        .arg(std::env::current_exe()?)
        .args(["--exact", TEST])
        .env("VELNOR_IMAGE_ATTESTATION_CHILD", "1")
        .env("VELNOR_ATTESTATION_TEST_DIR", fixtures.join("image-job"))
        .env(
            "VELNOR_TEST_ATTESTATION_ROOT",
            fixtures.join("historical/trusted-root.jsonl"),
        )
        .env("GH_TOKEN", "offline-fixture")
        .env_remove("VELNOR_GH_FIXTURE_WORKER");
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(30))?;
    let (output, _) = velnor_actions_mise::command::run_prepared_quiescent_until(
        &mut command,
        16 * 1024,
        deadline,
    )?;
    assert!(!output.success, "forged host child marker was accepted");
    assert!(String::from_utf8_lossy(&output.stdout).contains("FAILED"));
    Ok(())
}

fn probe_invalid_nonces() -> Result<(), Box<dyn std::error::Error>> {
    let nonce = std::env::var("VELNOR_GH_FIXTURE_NONCE")?;
    let receipt = std::path::PathBuf::from(format!("/target/receipt-{nonce}"));
    assert!(!receipt.exists());
    for value in [None, Some("invalid")] {
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command
            .env("VELNOR_IMAGE_NONCE_PROBE", "1")
            .args(["--exact", TEST]);
        if let Some(value) = value {
            command.env("VELNOR_GH_FIXTURE_NONCE", value);
        } else {
            command.env_remove("VELNOR_GH_FIXTURE_NONCE");
        }
        let deadline = CheckDeadline::after(std::time::Duration::from_secs(30))?;
        let (output, _) = velnor_actions_mise::command::run_prepared_quiescent_until(
            &mut command,
            16 * 1024,
            deadline,
        )?;
        assert!(output.success, "invalid nonce probe failed");
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        assert!(!receipt.exists());
    }
    Ok(())
}

fn expectation() -> AttestationExpectation<'static> {
    AttestationExpectation {
        repository_uri: "https://github.com/tailrocks/velnor-new",
        owner_uri: "https://github.com/tailrocks",
        caller_workflow_uri: WORKFLOW,
        caller_workflow_sha: SOURCE,
        signer_workflow_uri: WORKFLOW,
        signer_workflow_sha: SOURCE,
        source_sha: SOURCE,
        source_ref: "refs/heads/main",
        run_id: 42_149_229_551,
        run_attempt: 1,
        repository_id: "1390620900".to_owned(),
        trigger: "push",
    }
}

fn verify_record() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(
        std::env::var_os("VELNOR_ATTESTATION_TEST_DIR").ok_or("fixture directory")?,
    );
    let record = root.join(RECORD);
    let bundle = root.join("job-image-candidate.json.intoto.jsonl");
    let deadline = CheckDeadline::after(std::time::Duration::from_secs(90))?;
    let identity = AttestationCommandIdentity {
        repository: "tailrocks/velnor-new",
        workflow_uri: WORKFLOW,
        workflow_sha: SOURCE,
        source_sha: SOURCE,
        source_ref: "refs/heads/main",
        source_token: "offline-fixture",
    };
    let bytes = verify_one_with_bundle(
        &record,
        &bundle,
        &identity,
        deadline,
        VerificationPolicy::Repository,
    )?;
    let subjects = BTreeMap::from([
        (
            "job-image.oci.tar".to_owned(),
            "621cb823d7e0d7edaf4e307da48fa7650b62b7d4a0796584489301110286f037".to_owned(),
        ),
        (
            RECORD.to_owned(),
            "41b0c6b72052e4a7f6532180112a7aa9129de552ae29b560931445fcca7844b2".to_owned(),
        ),
        (
            "job-image-native-source-closure.json".to_owned(),
            "4c4a70ff8d97094ac9d79a46391fa1c908709768ab5335fd43e53f5b5e2e6830".to_owned(),
        ),
    ]);
    check_identity(&bytes, &subjects)?;
    check_tampering(&record, &bundle, &identity, deadline)?;
    Ok(())
}

fn check_identity(
    bytes: &[u8],
    subjects: &BTreeMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut expected = expectation();
    validate_output_with_additional_names(bytes, subjects, &[], &expected)?;
    expected.run_id += 1;
    assert!(validate_output_with_additional_names(bytes, subjects, &[], &expected).is_err());
    expected = expectation();
    expected.run_attempt += 1;
    assert!(validate_output_with_additional_names(bytes, subjects, &[], &expected).is_err());
    expected = expectation();
    expected.repository_id = "123".to_owned();
    assert!(validate_output_with_additional_names(bytes, subjects, &[], &expected).is_err());
    expected = expectation();
    expected.signer_workflow_uri =
        "https://github.com/tailrocks/velnor-new/.github/workflows/other.yml@refs/heads/main";
    assert!(validate_output_with_additional_names(bytes, subjects, &[], &expected).is_err());
    expected = expectation();
    expected.source_sha = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    assert!(validate_output_with_additional_names(bytes, subjects, &[], &expected).is_err());
    expected = expectation();
    expected.source_ref = "refs/heads/unreviewed";
    assert!(validate_output_with_additional_names(bytes, subjects, &[], &expected).is_err());
    Ok(())
}

fn check_tampering(
    record: &std::path::Path,
    bundle: &std::path::Path,
    identity: &AttestationCommandIdentity<'_>,
    deadline: CheckDeadline,
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let altered = tmp.path().join(RECORD);
    let mut record_bytes = std::fs::read(record)?;
    record_bytes.push(b' ');
    std::fs::write(&altered, record_bytes)?;
    assert!(
        verify_one_with_bundle(
            &altered,
            bundle,
            identity,
            deadline,
            VerificationPolicy::Repository
        )
        .is_err()
    );
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(bundle)?)?;
    let signature = value["dsseEnvelope"]["signatures"][0]["sig"]
        .as_str()
        .ok_or("signature")?;
    let mut altered_signature = signature.as_bytes().to_vec();
    altered_signature[0] = if altered_signature[0] == b'A' {
        b'B'
    } else {
        b'A'
    };
    value["dsseEnvelope"]["signatures"][0]["sig"] = String::from_utf8(altered_signature)?.into();
    let forged = tmp.path().join("forged.intoto.jsonl");
    std::fs::write(&forged, serde_json::to_vec(&value)?)?;
    assert!(
        verify_one_with_bundle(
            record,
            &forged,
            identity,
            deadline,
            VerificationPolicy::Repository
        )
        .is_err()
    );
    Ok(())
}
