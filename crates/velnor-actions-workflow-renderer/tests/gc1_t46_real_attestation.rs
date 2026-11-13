// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn verified_attestations_bind_run_attempt_and_all_subjects() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    for (name, bytes, source, event, run, subjects) in [
        (
            "push",
            include_bytes!("fixtures/image-attestation/verified-push.json").as_slice(),
            "a3417b150ac2f477498cadc329cdf4e23aa5b54b",
            "push",
            42_149_229_551_u64,
            serde_json::json!({
                "job-image.oci.tar": "621cb823d7e0d7edaf4e307da48fa7650b62b7d4a0796584489301110286f037",
                "job-image-candidate.json": "41b0c6b72052e4a7f6532180112a7aa9129de552ae29b560931445fcca7844b2",
                "job-image-native-source-closure.json": "4c4a70ff8d97094ac9d79a46391fa1c908709768ab5335fd43e53f5b5e2e6830"
            }),
        ),
        (
            "dispatch",
            include_bytes!("fixtures/image-attestation/verified-dispatch.json").as_slice(),
            "a3df794ac7906b0ebbe9fd5013fa48b5525cf6fb",
            "workflow_dispatch",
            39_245_035_087_u64,
            serde_json::json!({
                "job-image.oci.tar": "c16aa15abe96f9b142a78e4ea8743db3884c48e8f97f7ea0ee423ad9303b7332",
                "job-image-candidate.json": "f78634e880c6cbddec4a0f417d5fce78f2d833d5348559f4adf57c8a2a9957de"
            }),
        ),
    ] {
        let output: serde_json::Value = serde_json::from_slice(bytes)?;
        let mut request = serde_json::json!({"output": output, "source": source, "event": event, "run_id": run, "run_attempt": 1, "subjects": subjects});
        expect_ok(
            &fixture,
            "attestation",
            &request.to_string(),
            name,
            "ATTESTATION-OK",
        )?;
        request["run_attempt"] = serde_json::json!(2);
        expect_rejection(
            &fixture,
            "attestation",
            &request.to_string(),
            name,
            "ATTESTATION-REJECTED",
            Some("Attestation certificate run identity is incorrect."),
        )?;
        request["run_attempt"] = serde_json::json!(1);
        request["subjects"]["job-image.oci.tar"] = serde_json::json!("f".repeat(64));
        expect_rejection(
            &fixture,
            "attestation",
            &request.to_string(),
            name,
            "ATTESTATION-REJECTED",
            Some("Attestation subjects are incorrect."),
        )?;
        request["subjects"] = subjects;
        reject_changed_subject_sets(&fixture, &request, name)?;
    }
    Ok(())
}

fn reject_changed_subject_sets(
    fixture: &Fixture,
    request: &serde_json::Value,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    for variation in ["missing", "duplicate", "extra"] {
        let mut changed = request.clone();
        let subjects = changed["output"][0]["verificationResult"]["statement"]["subject"]
            .as_array_mut()
            .ok_or("subjects missing")?;
        match variation {
            "missing" => {
                subjects.pop();
            }
            "duplicate" => subjects.push(subjects[0].clone()),
            "extra" => subjects
                .push(serde_json::json!({"name":"extra", "digest":{"sha256":"e".repeat(64)}})),
            _ => unreachable!(),
        }
        expect_rejection(
            fixture,
            "attestation",
            &changed.to_string(),
            name,
            "ATTESTATION-REJECTED",
            Some("Attestation subjects are incorrect."),
        )?;
    }
    Ok(())
}
