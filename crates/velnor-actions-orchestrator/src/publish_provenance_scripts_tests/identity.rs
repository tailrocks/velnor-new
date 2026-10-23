use serde_json::{Value, json};

use super::{DIR, SUBJECT, TestResult, fixture, run, sign, tools_present};
use velnor_actions_workflow_renderer::publish_scripts::provenance::{
    CosignPin, CosignPlatform, receipt_script, sign_script_for_platform, verify_script,
};

const GH: &str = "gh() { command gh \"$@\"; }";

fn certificate_result(repository_id: &Value, owner_id: &Value) -> Value {
    json!({
        "verificationResult": {
            "signature": {
                "certificate": {
                    "sourceRepositoryIdentifier": repository_id,
                    "sourceRepositoryOwnerIdentifier": owner_id
                }
            }
        }
    })
}

fn result(repository_id: &Value, owner_id: &Value) -> Value {
    json!([certificate_result(repository_id, owner_id)])
}

fn signer_rejects(fx: &super::Fixture, document: &Value) -> TestResult {
    let output = sign(
        fx,
        &super::hex_sha256(&fx.cosign)?,
        &[("FAKE_VERIFY_JSON", &document.to_string())],
    )?;
    assert!(
        !output.status.success(),
        "unbound verification accepted: {document}"
    );
    assert!(
        !fx.temp
            .join("provenance/libs-1.0.0-maven.zip.provenance.json")
            .exists(),
        "a rejected identity must not produce a receipt"
    );
    Ok(())
}

#[test]
fn signer_rejects_missing_noncanonical_or_mismatched_certificate_ids() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let rejected = [
        json!([]),
        Value::Null,
        json!([{}]),
        json!([{"verificationResult":{"signature":{"certificate":{"sourceRepositoryOwnerIdentifier":"654321"}}}}]),
        json!([{"verificationResult":{"signature":{"certificate":{"sourceRepositoryIdentifier":"123456"}}}}]),
        json!([{"verificationResult":{"signature":{"certificate":{"sourceRepositoryIdentifier":123_456,"sourceRepositoryOwnerIdentifier":"654321"}}}}]),
        json!([{"verificationResult":{"signature":{"certificate":{"sourceRepositoryIdentifier":"123456","sourceRepositoryOwnerIdentifier":654_321}}}}]),
        result(&json!("999999"), &json!("654321")),
        result(&json!("123456"), &json!("999999")),
        result(&json!("0123456"), &json!("654321")),
    ];
    for document in rejected {
        signer_rejects(&fixture()?, &document)?;
    }

    let multiple = json!([
        certificate_result(&json!("123456"), &json!("654321")),
        certificate_result(&json!("123456"), &json!("999999"))
    ]);
    signer_rejects(&fixture()?, &multiple)?;
    let fx = fixture()?;
    let pin = super::hex_sha256(&fx.cosign)?;
    let empty_trusted_id = sign(&fx, &pin, &[("GITHUB_REPOSITORY_ID", "")])?;
    assert!(!empty_trusted_id.status.success());
    Ok(())
}

#[test]
fn verifier_and_no_gh_receipt_check_bind_repository_and_owner_ids() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let fx = fixture()?;
    let cosign_sha = super::hex_sha256(&fx.cosign)?;
    assert!(sign(&fx, &cosign_sha, &[])?.status.success());

    let wrong_certificate = result(&json!("123456"), &json!("999999")).to_string();
    let verify = verify_script(GH, DIR, SUBJECT);
    let refused = run(&fx, &verify, &[("FAKE_VERIFY_JSON", &wrong_certificate)])?;
    assert!(!refused.status.success(), "wrong owner ID accepted");

    let receipt = receipt_script(DIR, SUBJECT);
    assert!(
        !run(&fx, &receipt, &[("GITHUB_REPOSITORY_ID", "999999")])?
            .status
            .success()
    );
    assert!(
        !run(&fx, &receipt, &[("GITHUB_REPOSITORY_OWNER_ID", "999999")])?
            .status
            .success()
    );
    assert!(
        !run(&fx, &receipt, &[("GITHUB_REPOSITORY", "acme/renamed")])?
            .status
            .success()
    );
    assert!(
        !run(&fx, &receipt, &[("GITHUB_REPOSITORY_ID", "0123456")])?
            .status
            .success()
    );
    assert!(
        !run(&fx, &receipt, &[("GITHUB_REPOSITORY_OWNER_ID", "")])?
            .status
            .success()
    );

    let receipt_path = fx
        .temp
        .join("provenance/libs-1.0.0-maven.zip.provenance.json");
    let mut saved: Value = serde_json::from_slice(&std::fs::read(&receipt_path)?)?;
    saved["source"]["repository_id"] = json!("999999");
    std::fs::write(&receipt_path, serde_json::to_vec(&saved)?)?;
    assert!(
        !run(&fx, &receipt, &[])?.status.success(),
        "tampered receipt identity accepted"
    );
    Ok(())
}

#[test]
fn architecture_selects_the_matching_cosign_asset_url() -> TestResult {
    let pin = CosignPin {
        version: "3.1.4",
        sha256: &"a".repeat(64),
        size: 1,
    };
    let arm =
        sign_script_for_platform("gh() { :; }", DIR, SUBJECT, pin, CosignPlatform::LinuxArm64)?;
    assert!(arm.contains("/cosign-linux-arm64'"), "{arm}");
    assert!(!arm.contains("/cosign-linux-amd64'"), "{arm}");
    assert!(
        arm.contains("wc -c < \"$w/cosign\""),
        "exact size check missing"
    );
    Ok(())
}
