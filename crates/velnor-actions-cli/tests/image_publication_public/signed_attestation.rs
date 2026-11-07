// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support::{Fixture, FixtureProfile, hex};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, fs};

const PAYLOAD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../velnor-actions-orchestrator/tests/attestation_fixtures/image-job/job-image-candidate.json"
));
const BUNDLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../velnor-actions-orchestrator/tests/attestation_fixtures/image-job/job-image-candidate.json.intoto.jsonl"
));

#[test]
fn public_publication_rejects_an_unverified_certificate_before_registry_mutation()
-> Result<(), Box<dyn Error>> {
    let scenario = "signed_attestation::public_publication_rejects_an_unverified_certificate_before_registry_mutation";
    if !super::begin_isolated(scenario)? {
        return Ok(());
    }
    let valid: Value = serde_json::from_slice(BUNDLE)?;
    let mut invalid = valid.clone();
    let certificate = invalid
        .pointer_mut("/verificationMaterial/certificate/rawBytes")
        .ok_or("certificate fixture")?;
    let mut bytes = certificate
        .as_str()
        .ok_or("certificate bytes")?
        .as_bytes()
        .to_vec();
    let index = bytes.len().checked_sub(8).ok_or("short certificate")?;
    bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
    *certificate = String::from_utf8(bytes)?.into();
    for (bundle, expected) in [
        (
            vec![valid.clone()],
            "package_attestation_run_identity_mismatch",
        ),
        (
            vec![invalid.clone()],
            "package_attestation_verification_failed",
        ),
        (
            vec![invalid.clone(), valid.clone()],
            "package_attestation_verification_failed",
        ),
        (
            vec![valid, invalid],
            "package_attestation_verification_failed",
        ),
    ] {
        assert_rejected_without_mutation(bundle, expected)?;
    }
    super::isolated_worker::record_success("image_publication", scenario)?;
    Ok(())
}

fn assert_rejected_without_mutation(
    bundles: Vec<Value>,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_for(FixtureProfile::Job)?;
    fs::write(&fixture.archive, PAYLOAD)?;
    let mut record: Value = serde_json::from_slice(&fs::read(&fixture.record)?)?;
    record["archive"]["sha256"] = hex(&Sha256::digest(PAYLOAD)).into();
    record["archive"]["size_bytes"] = PAYLOAD.len().into();
    fs::write(&fixture.record, serde_json::to_vec(&record)?)?;
    let pages = bundles
        .into_iter()
        .map(|bundle| json!({"attestations":[{"bundle":bundle}]}))
        .collect::<Vec<_>>();
    fixture.set_attestation_pages(json!(pages))?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    let archive_before = fs::read(&fixture.archive)?;
    let record_before = fs::read(&fixture.record)?;
    let registry_before = fixture.preseed_registry_state()?;
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "expected {expected}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(!fixture.receipt.exists());
    assert_eq!(fixture.registry_state()?, registry_before);
    assert_eq!(fs::read(&fixture.archive)?, archive_before);
    assert_eq!(fs::read(&fixture.record)?, record_before);
    assert_eq!(fs::read(&foreign)?, b"preserve");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    Ok(())
}
