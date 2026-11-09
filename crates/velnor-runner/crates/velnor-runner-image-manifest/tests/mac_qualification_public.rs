// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::error::Error;
use velnor_runner_image_manifest::mac_qualification::parse_mac_runner_qualification;

fn image(role: &str) -> Value {
    json!({"repository": format!("ghcr.io/tailrocks/velnor-{role}"), "registry_digest": format!("sha256:{}", "a".repeat(64)), "platform_manifest_digest": format!("sha256:{}", "a".repeat(64)), "config_digest": format!("sha256:{}", "b".repeat(64)), "local_id": format!("sha256:{}", "c".repeat(64))})
}

fn receipt() -> Value {
    json!({"schema": "velnor.runner-image-mac-qualification.v1", "source_commit": "a".repeat(40), "run_id": 1, "run_attempt": 1, "publisher_artifact_id": 1, "manifest_sha256": "a".repeat(64), "runner_lock_sha256": "b".repeat(64), "platform": "linux/amd64", "images": {"runner": image("runner"), "dind": image("dind"), "resource_probe": image("resource-probe")}, "outer_daemon": {"id": "outer", "root": "/var/lib/docker", "store": "containerd"}, "inner_daemon_id": "inner", "versions": {"runner": "1", "docker": "2", "buildx": "3", "compose": "4", "node": "5"}, "cleanup_complete": true})
}

#[test]
fn mac_receipt_requires_closed_identity_and_successful_cleanup() -> Result<(), Box<dyn Error>> {
    let valid = receipt();
    let bytes = serde_json::to_vec(&valid)?;
    let parsed =
        parse_mac_runner_qualification(&bytes).map_err(|_| "valid Mac receipt rejected")?;
    assert_eq!(
        parsed.images.resource_probe.repository,
        "ghcr.io/tailrocks/velnor-resource-probe"
    );
    let raw = String::from_utf8(bytes)?;
    let duplicate = raw.replacen('{', "{\"run_id\":42,", 1);
    assert!(
        parse_mac_runner_qualification(duplicate.as_bytes()).is_err(),
        "duplicate run ID accepted"
    );
    for (path, value) in [
        (
            "/images/runner/repository",
            json!("ghcr.io/tailrocks/wrong"),
        ),
        ("/images/dind/registry_digest", json!("sha256:wrong")),
        ("/inner_daemon_id", json!("outer")),
        ("/outer_daemon/store", json!("invalid\nstore")),
        ("/cleanup_complete", json!(false)),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(path).ok_or("fixture path missing")? = value;
        assert!(
            parse_mac_runner_qualification(&serde_json::to_vec(&changed)?).is_err(),
            "invalid {path} accepted"
        );
    }
    let mut unknown = valid;
    unknown
        .as_object_mut()
        .ok_or("fixture object missing")?
        .insert("extra".into(), json!(true));
    assert!(
        parse_mac_runner_qualification(&serde_json::to_vec(&unknown)?).is_err(),
        "unknown field accepted"
    );
    Ok(())
}

#[test]
fn signed_envelope_binds_exact_native_receipt_and_both_authorities() -> Result<(), Box<dyn Error>> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    use velnor_runner_image_manifest::mac_attestation::parse_mac_qualification_attestation;

    let native = serde_json::to_vec(&receipt())?;
    let digits = b"0123456789abcdef";
    let hash = Sha256::digest(&native)
        .iter()
        .flat_map(|byte| {
            [
                char::from(digits[usize::from(byte >> 4)]),
                char::from(digits[usize::from(byte & 15)]),
            ]
        })
        .collect::<String>();
    let envelope = json!({
        "schema":"velnor.runner-image-mac-attestation.v1",
        "public_authority":{"repository":"tailrocks/velnor-new","repository_id":1_390_620_900,"source_commit":"a".repeat(40),"release_run_id":1,"release_run_attempt":1,"producer_artifact_id":9,"publisher_artifact_id":1},
        "companion":{"repository":"donbeave/velnor-mac-qualification","repository_id":1_450_917_587,"owner_id":139_017,"workflow_path":".github/workflows/qualify.yml","workflow_ref":"refs/heads/main","workflow_sha":"c".repeat(40),"run_id":2,"run_attempt":1,"qualification_job_id":3,"runner_id":4,"receipt_artifact_id":5,"receipt_artifact_digest":format!("sha256:{}","d".repeat(64))},
        "sdk":{"repository":"tailrocks/velnor-new","version":"1.2.3","source_commit":"e".repeat(40),"authority":{"channel":"stable","release_manifest_sha256":"f".repeat(64)},"target":"aarch64-apple-darwin","asset_sha256":"a".repeat(64)},
        "qualification":{"receipt_base64":STANDARD.encode(&native),"native_receipt_sha256":hash}
    });
    let bytes = serde_json::to_vec(&envelope)?;
    let (_, parsed) = parse_mac_qualification_attestation(&bytes)
        .map_err(|_| "valid signed envelope rejected")?;
    assert_eq!(parsed.publisher_artifact_id, 1);
    check_preview_authority(&envelope)?;
    let raw = String::from_utf8(bytes)?;
    let duplicate = raw.replacen('{', "{\"schema\":\"duplicate\",", 1);
    assert!(
        parse_mac_qualification_attestation(duplicate.as_bytes()).is_err(),
        "duplicate envelope schema accepted"
    );
    for (path, value) in [
        ("/public_authority/repository_id", json!(1)),
        ("/public_authority/source_commit", json!("b".repeat(40))),
        ("/public_authority/release_run_id", json!(2)),
        ("/public_authority/publisher_artifact_id", json!(2)),
        ("/companion/owner_id", json!(1)),
        ("/companion/workflow_ref", json!("refs/heads/other")),
        ("/sdk/target", json!("x86_64-unknown-linux-gnu")),
        ("/qualification/receipt_base64", json!("!")),
        (
            "/qualification/native_receipt_sha256",
            json!("f".repeat(64)),
        ),
    ] {
        let mut changed = envelope.clone();
        *changed
            .pointer_mut(path)
            .ok_or("envelope fixture path missing")? = value;
        assert!(
            parse_mac_qualification_attestation(&serde_json::to_vec(&changed)?).is_err(),
            "invalid {path} accepted"
        );
    }
    let mut unknown = envelope;
    unknown
        .as_object_mut()
        .ok_or("envelope fixture object missing")?
        .insert("extra".into(), json!(true));
    assert!(
        parse_mac_qualification_attestation(&serde_json::to_vec(&unknown)?).is_err(),
        "unknown envelope field accepted"
    );
    Ok(())
}

fn check_preview_authority(envelope: &serde_json::Value) -> Result<(), Box<dyn Error>> {
    use velnor_runner_image_manifest::mac_attestation::parse_mac_qualification_attestation;
    let mut preview = envelope.clone();
    *preview
        .pointer_mut("/sdk/authority")
        .ok_or("sdk authority missing")? = json!({
        "channel": "source-preview", "receipt_sha256": "1".repeat(64),
        "bundle_sha256": "2".repeat(64), "authority_sha": "3".repeat(40), "release_id": 4
    });
    assert!(
        parse_mac_qualification_attestation(&serde_json::to_vec(&preview)?).is_ok(),
        "valid preview identity rejected"
    );
    let mut confused = preview.clone();
    confused
        .pointer_mut("/sdk/authority")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("sdk authority missing")?
        .insert("release_manifest_sha256".into(), json!("f".repeat(64)));
    assert!(
        parse_mac_qualification_attestation(&serde_json::to_vec(&confused)?).is_err(),
        "mixed SDK authorities accepted"
    );
    *preview
        .pointer_mut("/sdk/authority/release_id")
        .ok_or("release ID missing")? = json!(0);
    assert!(
        parse_mac_qualification_attestation(&serde_json::to_vec(&preview)?).is_err(),
        "zero preview release ID accepted"
    );
    Ok(())
}
