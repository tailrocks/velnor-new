// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/worker/guest_image/identity.rs"]
mod identity;

use identity::{RegistryInput, registry};
use velnor_runner_image_manifest::ImageKind;

#[test]
fn registry_identity_binds_role_source_and_release_without_archive_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "a".repeat(40);
    let release = "b".repeat(64);
    let manifest = format!("sha256:{}", "c".repeat(64));
    let config = format!("sha256:{}", "d".repeat(64));
    let mut input = RegistryInput {
        role: ImageKind::Runner,
        expected_role: ImageKind::Runner,
        source: &source,
        expected_source: &source,
        release_manifest_sha256: &release,
        expected_release_manifest_sha256: &release,
        registry_digest: &manifest,
        manifest_digest: &manifest,
        config_digest: &config,
    };
    let value = serde_json::to_value(registry(&input).ok_or("valid registry identity rejected")?)?;
    assert_eq!(
        value,
        serde_json::json!({"origin":"registry", "repository":"ghcr.io/tailrocks/velnor-runner", "release_manifest_sha256":release, "registry_digest":manifest, "manifest_digest":manifest, "config_digest":config})
    );
    input.expected_role = ImageKind::Dind;
    assert!(registry(&input).is_none(), "wrong role accepted");
    input.expected_role = ImageKind::Runner;
    input.expected_source = "0000000000000000000000000000000000000000";
    assert!(registry(&input).is_none(), "wrong source accepted");
    input.expected_source = &source;
    input.expected_release_manifest_sha256 =
        "0000000000000000000000000000000000000000000000000000000000000000";
    assert!(registry(&input).is_none(), "wrong release accepted");
    Ok(())
}
