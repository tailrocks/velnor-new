// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;

use serde_json::{Value, json};

use super::*;

const IDENTITY: &str = include_str!("../src/runner_image_authority_identity.py");
const CANDIDATE: &str = include_str!("../src/runner_image_authority_candidate.py");
const CUSTODY: &str = include_str!("../src/runner_image_authority_custody.py");
const PUBLICATION: &str = include_str!("../src/runner_image_authority_publish.py");
const OCI: &str = include_str!("../src/job_image_authority_oci.py");
const REGISTRY: &str = include_str!("../src/runner_shared_job_image_authority_registry.py");
const REGISTRY_PUBLISH: &str = include_str!("../src/runner_shared_job_image_authority_publish.py");
const TRANSPORT: &str = include_str!("../src/runner_shared_job_image_authority_transport.py");
const DRIVER: &str = include_str!("../src/runner_image_authority_driver.py");
const FAMILY_FIXTURE: &str = include_str!("runner_family_fixed_publication_fixture.py");

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

fn family_fixture(scenario: &str) -> Result<(ImageParts, Value, Vec<u8>), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    for (name, source) in [
        ("runner_image_authority_driver.py", DRIVER),
        ("runner_image_authority_identity.py", IDENTITY),
        ("runner_image_authority_candidate.py", CANDIDATE),
        ("runner_image_authority_custody.py", CUSTODY),
        ("runner_image_authority_publish.py", PUBLICATION),
        ("job_image_authority_oci.py", OCI),
        ("job_image_authority_registry.py", REGISTRY),
        ("job_image_authority_publish.py", REGISTRY_PUBLISH),
        ("job_image_authority_transport.py", TRANSPORT),
    ] {
        std::fs::write(root.path().join(name), source)?;
    }
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    std::fs::write(root.path().join("candidate.oci.tar"), archive)?;
    std::fs::write(
        root.path().join("runner.lock"),
        include_bytes!("../../../.velnor/runner.lock"),
    )?;
    std::fs::write(root.path().join("fixture.py"), FAMILY_FIXTURE)?;
    std::fs::create_dir(root.path().join("private"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            root.path().join("private"),
            std::fs::Permissions::from_mode(0o700),
        )?;
    }
    let input = json!({
        "scenario": scenario,
        "parts": {
            "index": hex_encode(&parts.index),
            "manifest": hex_encode(&parts.manifest),
            "config": hex_encode(&parts.config),
            "layer": hex_encode(&parts.layer)
        }
    });
    std::fs::write(root.path().join("case.json"), serde_json::to_vec(&input)?)?;
    let output = Command::new("python3")
        .arg(root.path().join("fixture.py"))
        .arg(root.path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = serde_json::from_slice(&std::fs::read(root.path().join("result.json"))?)?;
    let manifest_raw = match std::fs::read(root.path().join("manifest.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    Ok((parts, result, manifest_raw))
}

fn assert_family_manifest(manifest: &Value, tags: &Value, parts: &ImageParts) {
    assert_eq!(manifest["schema_version"], 2);
    assert_eq!(manifest["repository"], "tailrocks/velnor-new");
    assert_eq!(manifest["source_ref"], "refs/heads/main");
    assert_eq!(manifest["source_commit"], "a".repeat(40));
    assert_eq!(
        manifest["signer_workflow"],
        ".github/workflows/product-release-images.yml"
    );
    assert_eq!(manifest["signer_ref"], "refs/heads/main");
    assert_eq!(manifest["workflow_authority_sha"], "a".repeat(40));
    assert_eq!(
        manifest["runner_lock_sha256"],
        sha256_hex(include_bytes!("../../../.velnor/runner.lock"))
    );
    assert_eq!(manifest["versions"]["runner"], "2.338.0");
    assert_eq!(manifest["versions"]["docker"], "29.9.1");
    assert_eq!(manifest["versions"]["buildx"], "0.38.0");
    assert_eq!(manifest["versions"]["compose"], "5.6.0");
    assert_eq!(manifest["versions"]["node"], "26.12.0");
    let expected_manifest_digest = format!("sha256:{}", sha256_hex(&parts.manifest));
    for (role, repository, context) in [
        (
            "runner",
            "ghcr.io/tailrocks/velnor-runner",
            "images/runner/ubuntu-26.04",
        ),
        ("dind", "ghcr.io/tailrocks/velnor-dind", "images/dind"),
        (
            "resource_probe",
            "ghcr.io/tailrocks/velnor-resource-probe",
            "images/resource-probe",
        ),
    ] {
        let image = &manifest["images"][role];
        assert_eq!(image["repository"], repository);
        assert_eq!(image["tag"], format!("sha-{}", "a".repeat(40)));
        assert_eq!(image["registry_digest"], expected_manifest_digest);
        assert_eq!(image["platform_manifest_digest"], expected_manifest_digest);
        assert_eq!(tags[role], sha256_hex(&parts.manifest));
        assert_eq!(
            image["config_digest"],
            format!("sha256:{}", sha256_hex(&parts.config))
        );
        assert_eq!(image["build_source"]["source_commit"], "a".repeat(40));
        assert_eq!(image["build_source"]["context_path"], context);
        assert_eq!(
            image["build_source"]["named_contexts"]["resource_probe"],
            "b".repeat(64)
        );
    }
    assert_eq!(manifest["resource_probe"]["protocol_version"], 1);
}

#[test]
fn family_coordinator_publishes_three_authenticated_images_and_serializes_release_manifest()
-> Result<(), Box<dyn Error>> {
    let (parts, result, manifest_raw) = family_fixture("publish")?;
    assert_eq!(result["ok"], true);
    assert!(manifest_raw.ends_with(b"\n"));
    let manifest: Value = serde_json::from_slice(&manifest_raw)?;
    assert_family_manifest(&manifest, &result["source_tags"], &parts);
    Ok(())
}

#[test]
fn family_reads_all_graphs_with_anonymous_pull_tokens() -> Result<(), Box<dyn Error>> {
    let (_, public, manifest) = family_fixture("publish")?;
    assert_eq!(public["ok"], true);
    assert!(!manifest.is_empty());
    let reads = public["anonymous_reads"]
        .as_array()
        .ok_or("missing anonymous reads")?;
    let requests = public["anonymous_token_requests"]
        .as_array()
        .ok_or("missing anonymous tokens")?;
    for role in ["runner", "dind", "resource-probe"] {
        let prefix = format!("/v2/tailrocks/velnor-{role}/");
        let paths: Vec<&str> = reads
            .iter()
            .filter_map(Value::as_str)
            .filter(|path| path.starts_with(&prefix))
            .collect();
        assert!(paths.iter().any(|path| path.contains("/manifests/sha256:")));
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.contains("/blobs/sha256:"))
                .count(),
            2
        );
        assert!(
            requests
                .iter()
                .filter_map(Value::as_str)
                .any(|request| request.contains(&format!("velnor-{role}%3Apull"))
                    && !request.contains("push"))
        );
    }
    Ok(())
}

#[test]
fn family_rejects_private_runner_before_promoting_tags() -> Result<(), Box<dyn Error>> {
    assert_anonymous_failure("private_runner")
}

#[test]
fn family_rejects_private_dind_before_promoting_tags() -> Result<(), Box<dyn Error>> {
    assert_anonymous_failure("private_dind")
}

#[test]
fn family_rejects_private_resource_probe_before_promoting_tags() -> Result<(), Box<dyn Error>> {
    assert_anonymous_failure("private_resource_probe")
}

#[test]
fn family_rejects_corrupt_anonymous_blob_before_promoting_tags() -> Result<(), Box<dyn Error>> {
    assert_anonymous_failure("anonymous_bad_blob")
}

fn assert_anonymous_failure(scenario: &str) -> Result<(), Box<dyn Error>> {
    let (_, result, manifest) = family_fixture(scenario)?;
    assert_eq!(result["ok"], false, "{scenario}");
    assert!(manifest.is_empty(), "{scenario}");
    assert!(
        result["registry_puts"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert!(
        result["source_tags"]
            .as_object()
            .is_some_and(|tags| tags.values().all(Value::is_null)),
        "{scenario}"
    );
    Ok(())
}

fn assert_no_publication(result: &Value, manifest_raw: &[u8], error: &str) {
    assert_eq!(result["ok"], false);
    assert_eq!(result["error"], error);
    assert_eq!(result["registry_puts"], 0);
    assert_eq!(result["stored_paths"], json!([]));
    assert_eq!(manifest_raw, Vec::<u8>::new());
}

#[test]
fn family_coordinator_rejects_bad_signer_and_third_role_bytes_before_registry_writes()
-> Result<(), Box<dyn Error>> {
    for (scenario, error) in [
        (
            "invalid_signer",
            "Qualification certificate identity does not match.",
        ),
        (
            "tampered_third_record",
            "Candidate record differs from qualified bytes.",
        ),
        (
            "different_probe_context",
            "Qualified runner-family source identities differ.",
        ),
    ] {
        let (_, result, manifest_raw) = family_fixture(scenario)?;
        assert_no_publication(&result, &manifest_raw, error);
    }
    Ok(())
}

#[test]
fn family_coordinator_rejects_third_archive_bytes_before_registry_writes()
-> Result<(), Box<dyn Error>> {
    let (parts, valid, valid_manifest_raw) = family_fixture("publish")?;
    assert_eq!(valid["ok"], true);
    assert!(
        valid["registry_puts"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    let valid_manifest: Value = serde_json::from_slice(&valid_manifest_raw)?;
    assert_family_manifest(&valid_manifest, &valid["source_tags"], &parts);

    let (_, rejected, manifest_raw) = family_fixture("tampered_third_archive")?;
    let mutation = &rejected["archive_mutation"];
    assert_eq!(
        mutation["qualified_size_bytes"],
        mutation["resource_probe_size_bytes"]
    );
    assert_eq!(mutation["runner_sha256"], mutation["qualified_sha256"]);
    assert_eq!(mutation["dind_sha256"], mutation["qualified_sha256"]);
    assert_ne!(
        mutation["resource_probe_sha256"],
        mutation["qualified_sha256"]
    );
    assert_eq!(rejected["registry_puts"], 0);
    assert_eq!(rejected["stored_paths"], json!([]));
    assert_eq!(rejected["ok"], false);
    assert_eq!(rejected["error"], "Artifact differs from its signed bytes.");
    assert_eq!(manifest_raw, Vec::<u8>::new());
    Ok(())
}

#[test]
fn family_coordinator_stops_when_current_main_changes_at_publication_boundaries()
-> Result<(), Box<dyn Error>> {
    let (_, initial, initial_manifest) = family_fixture("stale_before_publish")?;
    assert_no_publication(
        &initial,
        &initial_manifest,
        "Publication source is no longer current main.",
    );
    let (_, after_image, final_manifest) = family_fixture("stale_after_image")?;
    assert_eq!(after_image["ok"], false);
    assert_eq!(
        after_image["error"],
        "Publication source is no longer current main."
    );
    assert!(
        after_image["registry_puts"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert_eq!(final_manifest, Vec::<u8>::new());
    assert_eq!(after_image["source_tags"]["runner"], Value::Null);
    assert_eq!(after_image["source_tags"]["dind"], Value::Null);
    assert_eq!(after_image["source_tags"]["resource_probe"], Value::Null);
    let paths = after_image["stored_paths"]
        .as_array()
        .ok_or("stored paths missing")?;
    assert!(
        paths
            .iter()
            .filter_map(Value::as_str)
            .any(|path| { path.starts_with("/v2/tailrocks/velnor-runner/manifests/sha256:") })
    );
    assert!(!paths.iter().filter_map(Value::as_str).any(|path| {
        path.starts_with("/v2/tailrocks/velnor-dind/")
            || path.starts_with("/v2/tailrocks/velnor-resource-probe/")
    }));
    Ok(())
}

#[test]
fn family_coordinator_preserves_a_conflicting_source_tag() -> Result<(), Box<dyn Error>> {
    let (_, result, manifest_raw) = family_fixture("conflicting_source_tag")?;
    assert_eq!(result["ok"], false);
    assert_eq!(result["error"], "Registry digest HEAD verification failed.");
    assert!(
        result["registry_puts"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert_eq!(result["runner_tag_after_sha256"], result["preseed_sha256"]);
    assert_eq!(result["source_tags"]["runner"], result["preseed_sha256"]);
    assert_eq!(result["source_tags"]["dind"], Value::Null);
    assert_eq!(result["source_tags"]["resource_probe"], Value::Null);
    assert_eq!(manifest_raw, Vec::<u8>::new());
    Ok(())
}

#[test]
fn fixed_driver_preserves_existing_manifest_and_publishes_fresh_assets()
-> Result<(), Box<dyn Error>> {
    let (parts, fresh, raw) = family_fixture("driver_fresh")?;
    assert_eq!(fresh["driver_exit"], 0, "{fresh}");
    assert_eq!(fresh["files_valid"], true);
    let manifest: Value = serde_json::from_slice(&raw)?;
    assert_family_manifest(&manifest, &fresh["source_tags"], &parts);
    let (_, existing, _) = family_fixture("driver_existing")?;
    assert_eq!(existing["driver_exit"], 1, "{existing}");
    assert!(
        existing["driver_stderr"]
            .as_str()
            .is_some_and(|s| s.contains("FileExistsError"))
    );
    assert_eq!(existing["sentinel_preserved"], true);
    Ok(())
}
