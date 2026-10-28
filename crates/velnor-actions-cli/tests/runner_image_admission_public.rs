// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, error::Error, fs};

#[path = "runner_image_admission_public/support/mod.rs"]
mod support;

use sha2::{Digest, Sha256};
use support::{AdmissionFixture, oci};

fn lock_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in &digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}

#[test]
fn public_admission_writes_exact_descriptors_and_preserves_inputs() -> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::new()?;
    let before = fixture.images.bytes()?;
    let output = support::invoke(&fixture, &fixture.images)?;
    assert!(output.status.success(), "admission failed: {output:?}");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.output)?)?;
    assert_object_keys(
        &value,
        &[
            "schema_version",
            "runner_lock_sha256",
            "source_commit",
            "platform",
            "images",
        ],
    )?;
    assert_object_keys(&value["images"], &["runner", "dind", "resource_probe"])?;
    assert_eq!(value["schema_version"], 2);
    assert_eq!(
        value["runner_lock_sha256"],
        lock_sha256(&fs::read(fixture.root.join(".velnor/runner.lock"))?)
    );
    assert_eq!(value["source_commit"], fixture.source);
    assert_eq!(value["platform"], "linux/amd64");
    for (name, image, repository) in fixture.images.entries() {
        let descriptor = &value["images"][name];
        assert_object_keys(
            descriptor,
            &[
                "repository",
                "tag",
                "registry_digest",
                "platform_manifest_digest",
                "config_digest",
            ],
        )?;
        assert_eq!(descriptor["repository"], repository);
        assert_eq!(descriptor["tag"], format!("sha-{}", fixture.source));
        assert_eq!(descriptor["registry_digest"], image.manifest_digest);
        assert_eq!(
            descriptor["platform_manifest_digest"],
            image.manifest_digest
        );
        assert_eq!(descriptor["config_digest"], image.config_digest);
        assert_ne!(descriptor["registry_digest"], descriptor["config_digest"]);
    }
    assert_eq!(fixture.images.bytes()?, before);

    let existing = b"keep-existing-output";
    fs::write(&fixture.output, existing)?;
    let rejected = support::invoke(&fixture, &fixture.images)?;
    assert!(!rejected.status.success());
    assert_eq!(fs::read(&fixture.output)?, existing);
    Ok(())
}

#[test]
fn source_admission_rejects_dirty_checkout_and_hidden_lock_changes() -> Result<(), Box<dyn Error>> {
    for mutation in ["worktree", "index", "untracked"] {
        let fixture = AdmissionFixture::new()?;
        fixture.mutate_checkout(mutation)?;
        let output = support::invoke(&fixture, &fixture.images)?;
        assert!(!output.status.success(), "accepted {mutation} changes");
        assert!(!fixture.output.exists());
    }
    for index_flag in ["--skip-worktree", "--assume-unchanged"] {
        let fixture = AdmissionFixture::new()?;
        fixture.hide_and_change_lock(index_flag)?;
        let output = support::invoke(&fixture, &fixture.images)?;
        assert!(
            !output.status.success(),
            "accepted lock change hidden by {index_flag}: {output:?}"
        );
        assert!(!fixture.output.exists());
    }
    let mut fixture = AdmissionFixture::new()?;
    fixture.remove_source_lock_and_create_ignored_replacement()?;
    let output = support::invoke(&fixture, &fixture.images)?;
    let error = String::from_utf8(output.stderr)?;
    assert!(
        !output.status.success() && error.contains("runner_image_source_file"),
        "ignored replacement did not fail selected source blob lookup: {error}"
    );
    assert!(!fixture.output.exists());
    Ok(())
}

#[test]
fn replacement_ref_keeps_physical_source_a_admissible() -> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::new()?;
    let source = fixture.source.clone();
    let lock = fs::read(fixture.root.join(".velnor/runner.lock"))?;
    fixture.add_replacement_ref(false)?;

    let output = support::invoke(&fixture, &fixture.images)?;
    assert!(output.status.success(), "admission failed: {output:?}");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&fixture.output)?)?;
    assert_eq!(value["source_commit"], source);
    assert_eq!(value["runner_lock_sha256"], lock_sha256(&lock));
    for (name, image, _) in fixture.images.entries() {
        let descriptor = &value["images"][name];
        assert_eq!(descriptor["tag"], format!("sha-{source}"));
        assert_eq!(descriptor["registry_digest"], image.manifest_digest);
    }
    Ok(())
}

#[test]
fn replacement_ref_does_not_admit_physical_source_b_as_source_a() -> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::new()?;
    fixture.add_replacement_ref(true)?;

    let output = support::invoke(&fixture, &fixture.images)?;
    assert!(!output.status.success());
    assert!(!fixture.output.exists());
    Ok(())
}

#[test]
fn source_admission_rejects_skip_worktree_runner_dockerfile_change() -> Result<(), Box<dyn Error>> {
    reject_hidden_runner_dockerfile_change("--skip-worktree")
}

#[test]
fn source_admission_rejects_assume_unchanged_runner_dockerfile_change() -> Result<(), Box<dyn Error>>
{
    reject_hidden_runner_dockerfile_change("--assume-unchanged")
}

#[test]
fn selected_source_must_be_the_clean_checkout_head() -> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::new()?;
    fixture.advance_head()?;
    let output = support::invoke(&fixture, &fixture.images)?;
    assert!(!output.status.success());
    assert!(!fixture.output.exists());
    Ok(())
}

#[test]
fn source_commit_must_be_lowercase_full_sha_and_output_must_be_external()
-> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::new()?;
    for source in ["not-a-source-sha".to_owned(), "A".repeat(40)] {
        let output = support::invoke_with_source(&fixture, &fixture.images, &source)?;
        let error = String::from_utf8(output.stderr)?;
        assert!(
            !output.status.success() && error.contains("runner_image_source_commit"),
            "accepted invalid source SHA {source}: {error}"
        );
        assert!(!fixture.output.exists());
    }
    let inside = fixture.root.join("candidate-descriptors.json");
    let output = support::invoke_to(&fixture, &fixture.images, &fixture.source, &inside)?;
    let error = String::from_utf8(output.stderr)?;
    assert!(
        !output.status.success() && error.contains("runner_image_output_path"),
        "accepted output inside source root: {error}"
    );
    assert!(!inside.exists());
    Ok(())
}

#[test]
fn all_locked_versions_must_be_present_string_and_trimmed() -> Result<(), Box<dyn Error>> {
    let mut fixture = AdmissionFixture::new()?;
    let pins = [
        "runner_version",
        "docker_version",
        "buildx_version",
        "compose_version",
        "node_version",
    ];
    for pin in pins {
        for malformed in [
            "missing",
            "number",
            "empty",
            "leading_whitespace",
            "trailing_whitespace",
        ] {
            let lock = support::lock_with_mutation(pin, malformed);
            fixture.commit_lock(&lock)?;
            let output = support::invoke(&fixture, &fixture.images)?;
            let error = String::from_utf8(output.stderr)?;
            assert!(
                !output.status.success() && error.contains("runner_image_lock"),
                "accepted {malformed} {pin}: {error}"
            );
            assert!(!fixture.output.exists());
        }
    }
    Ok(())
}

#[test]
fn locked_versions_must_be_exact_semver() -> Result<(), Box<dyn Error>> {
    let mut fixture = AdmissionFixture::new()?;
    for pin in [
        "runner_version",
        "docker_version",
        "buildx_version",
        "compose_version",
        "node_version",
    ] {
        for malformed in ["latest", "malformed"] {
            let lock = support::lock_with_mutation(pin, malformed);
            fixture.commit_lock(&lock)?;
            let expected_lock = fs::read(fixture.root.join(".velnor/runner.lock"))?;
            let expected_archives = fixture.images.bytes()?;
            let output = support::invoke(&fixture, &fixture.images)?;
            let error = String::from_utf8(output.stderr)?;
            assert!(
                !output.status.success() && error.contains("runner_image_lock_version"),
                "accepted {malformed} {pin}: {error}"
            );
            assert!(!fixture.output.exists());
            assert_eq!(
                fs::read(fixture.root.join(".velnor/runner.lock"))?,
                expected_lock
            );
            assert_eq!(fixture.images.bytes()?, expected_archives);
        }
    }
    Ok(())
}

#[test]
fn each_image_must_match_source_platform_and_authenticated_graph() -> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::new()?;
    for (slot, name) in [(0, "runner"), (1, "dind"), (2, "resource_probe")] {
        for failure in ["revision", "platform", "graph"] {
            let mut images = fixture.images.clone();
            let variant = match failure {
                "revision" => oci::for_kind(
                    &fixture.image_dir,
                    name,
                    &fixture.source,
                    Some("wrong"),
                    None,
                    false,
                )?,
                "platform" => oci::for_kind(
                    &fixture.image_dir,
                    name,
                    &fixture.source,
                    None,
                    Some("arm64"),
                    false,
                )?,
                "graph" => {
                    oci::for_kind(&fixture.image_dir, name, &fixture.source, None, None, true)?
                }
                _ => unreachable!(),
            };
            images.replace(slot, variant);
            let output = support::invoke(&fixture, &images)?;
            assert!(
                !output.status.success(),
                "accepted {failure} mismatch for {name}"
            );
            assert!(!fixture.output.exists());
        }
    }
    Ok(())
}

#[test]
fn probe_profile_is_exact_while_runner_and_dind_remain_flexible() -> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::new()?;
    for mutation in [
        "user",
        "working_directory",
        "entrypoint",
        "cmd",
        "cmd_null",
        "env",
        "label",
        "key",
    ] {
        let image = oci::probe_mutation(&fixture.image_dir, &fixture.source, mutation)?;
        let mut images = fixture.images.clone();
        images.resource_probe = image;
        let output = support::invoke(&fixture, &images)?;
        assert!(
            !output.status.success(),
            "accepted resource-probe profile mutation {mutation}"
        );
        assert!(!fixture.output.exists());
    }
    Ok(())
}

fn assert_object_keys(value: &serde_json::Value, expected: &[&str]) -> Result<(), Box<dyn Error>> {
    let object = value.as_object().ok_or("expected JSON object")?;
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    Ok(())
}

fn reject_hidden_runner_dockerfile_change(index_flag: &str) -> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::new()?;
    let changed_bytes = fixture.hide_and_change_runner_dockerfile(index_flag)?;
    let output = support::invoke(&fixture, &fixture.images)?;
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && error.contains("runner_image_source_index_flags"),
        "accepted Runner Dockerfile change hidden by {index_flag}: status={:?} stderr={error}",
        output.status
    );
    assert!(!fixture.output.exists());
    fixture.preserve_hidden_runner_dockerfile_state(index_flag, &changed_bytes)?;
    Ok(())
}
