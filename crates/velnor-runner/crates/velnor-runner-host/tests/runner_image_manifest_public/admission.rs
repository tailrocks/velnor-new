// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn public_write_runner_manifest_rejects_config_digest_matching_manifest_for_every_role()
-> TestResult<()> {
    for role in ["runner", "dind", "resource_probe"] {
        let mut document = descriptors();
        document["images"][role]["config_digest"] =
            document["images"][role]["registry_digest"].clone();
        let temp = tempfile::tempdir()?;
        let bytes = serde_json::to_vec(&document)?;
        let (output, path) = invoke(&temp, &bytes, LOCK, SOURCE, SOURCE, None)?;
        assert!(
            !output.status.success(),
            "accepted identical digests for {role}"
        );
        assert!(!path.exists(), "created manifest for {role}");
    }
    Ok(())
}

#[test]
fn public_validate_runner_image_manifest_rejects_untrusted_claims() -> TestResult<()> {
    use velnor_runner_host::resource_probe_release::validate_runner_image_manifest;

    let temp = tempfile::tempdir()?;
    let descriptor_bytes = serde_json::to_vec(&descriptors())?;
    let (output, path) = invoke(&temp, &descriptor_bytes, LOCK, SOURCE, SOURCE, None)?;
    assert!(output.status.success());
    let bytes = fs::read(path)?;
    validate_runner_image_manifest(&bytes, SOURCE, LOCK.as_bytes())?;
    let original: Value = serde_json::from_slice(&bytes)?;
    let mut cases = Vec::new();
    for role in ["runner", "dind", "resource_probe"] {
        let mut wrong_repository = original.clone();
        wrong_repository["images"][role]["repository"] = json!("ghcr.io/other/image");
        cases.push(wrong_repository);
        let mut wrong_registry_digest = original.clone();
        wrong_registry_digest["images"][role]["registry_digest"] = json!("sha256:invalid");
        cases.push(wrong_registry_digest);
        let mut wrong_platform_digest = original.clone();
        wrong_platform_digest["images"][role]["platform_manifest_digest"] =
            json!(format!("sha256:{}", "9".repeat(64)));
        cases.push(wrong_platform_digest);
        let mut wrong_config_digest = original.clone();
        wrong_config_digest["images"][role]["config_digest"] =
            original["images"][role]["registry_digest"].clone();
        cases.push(wrong_config_digest);
    }
    for (key, value) in [
        ("source_commit", json!(OTHER_SOURCE)),
        ("signer_workflow", json!(".github/workflows/publish.yml")),
        ("workflow_authority_sha", json!(OTHER_SOURCE)),
        ("platform", json!("linux/arm64")),
        ("unexpected", json!(true)),
    ] {
        let mut wrong = original.clone();
        wrong[key] = value;
        cases.push(wrong);
    }
    for wrong in cases {
        assert!(
            validate_runner_image_manifest(&serde_json::to_vec(&wrong)?, SOURCE, LOCK.as_bytes())
                .is_err(),
            "accepted {wrong}"
        );
    }
    let duplicate = String::from_utf8(bytes)?.replacen(
        "\"schema_version\":2",
        "\"schema_version\":2,\"schema_version\":2",
        1,
    );
    assert!(validate_runner_image_manifest(duplicate.as_bytes(), SOURCE, LOCK.as_bytes()).is_err());
    assert!(
        validate_runner_image_manifest(
            &serde_json::to_vec(&original)?,
            SOURCE,
            LOCK.replace("2.338.0", "2.339.0").as_bytes()
        )
        .is_err()
    );
    Ok(())
}
