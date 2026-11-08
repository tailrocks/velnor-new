// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod error {
    pub(crate) use velnor_runner_host::HostError;
}
#[path = "../src/resource_probe_release/exact_semver.rs"]
mod exact_semver;
use exact_semver::is_exact_semver;
#[path = "../src/resource_probe_release/runner_manifest.rs"]
mod runner_manifest;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, error::Error};

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LOCK: &str = "runner_version = \"2.338.0\"\ndocker_version = \"29.9.1\"\nbuildx_version = \"0.38.0\"\ncompose_version = \"5.6.0\"\nnode_version = \"26.12.0\"\n";

type TestResult = Result<(), Box<dyn Error>>;

fn digest(bytes: &[u8]) -> String {
    let alphabet = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                char::from(alphabet[usize::from(byte >> 4)]),
                char::from(alphabet[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn image(repository: &str, marker: char, context: &str, names: &[&str]) -> Value {
    let manifest = format!("sha256:{}", marker.to_string().repeat(64));
    let contexts = names
        .iter()
        .map(|name| (*name, "f".repeat(64)))
        .collect::<BTreeMap<_, _>>();
    json!({"repository": repository, "tag": format!("sha-{SOURCE}"),
        "registry_digest": manifest, "platform_manifest_digest": manifest,
        "config_digest": format!("sha256:{}", "e".repeat(64)),
        "build_source": {"metadata_sha256": "d".repeat(64),
            "build_type": "https://github.com/moby/buildkit/blob/master/docs/attestations/slsa-definitions.md",
            "source_repository": "https://github.com/tailrocks/velnor-new", "source_commit": SOURCE,
            "context_path": context, "dockerfile_path": format!("{context}/Dockerfile"),
            "builder_platform": "linux/amd64", "named_contexts": contexts}})
}

fn manifest(lock: &str) -> Value {
    json!({"schema_version":2, "repository":"tailrocks/velnor-new", "source_ref":"refs/heads/main",
        "signer_workflow":".github/workflows/product-release-images.yml", "signer_ref":"refs/heads/main",
        "trusted_signing_identity":{"oidc_issuer":"https://token.actions.githubusercontent.com",
          "certificate_identity":"https://github.com/tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main"},
        "source_commit":SOURCE, "runner_lock_sha256":digest(lock.as_bytes()), "workflow_authority_sha":SOURCE,
        "platform":"linux/amd64", "versions":{"runner":"2.338.0", "docker":"29.9.1", "buildx":"0.38.0", "compose":"5.6.0", "node":"26.12.0"},
        "images":{"runner":image("ghcr.io/tailrocks/velnor-runner",'1',"images/runner/ubuntu-26.04", &["resource_probe","storage_init","job_start_hook","job_started_hook"]),
          "dind":image("ghcr.io/tailrocks/velnor-dind",'2',"images/dind", &["resource_probe"]),
          "resource_probe":image("ghcr.io/tailrocks/velnor-resource-probe",'3',"images/resource-probe", &["resource_probe"])},
        "resource_probe":{"protocol_version":1,"image_user":"65532:65532","entrypoint":"/velnor/resource-probe","working_directory":"/"}})
}

fn reject(bytes: &[u8], lock: &str, name: &str) {
    let original = bytes.to_vec();
    let result = runner_manifest::parse_for_source(bytes, SOURCE, lock.as_bytes());
    assert!(result.is_err(), "accepted invalid manifest: {name}");
    if let Err(error) = result {
        assert!(
            matches!(error, error::HostError::Identity),
            "{name}: {error:?}"
        );
    }
    assert_eq!(bytes, original, "mutated {name}");
}

#[test]
fn accepts_complete_bound_manifest() -> TestResult {
    let expected = manifest(LOCK);
    let bytes = serde_json::to_vec(&expected)?;
    let parsed = runner_manifest::parse_for_source(&bytes, SOURCE, LOCK.as_bytes())?;
    assert_eq!(serde_json::to_value(parsed)?, expected);
    Ok(())
}

#[test]
fn rejects_invalid_manifest_bindings_and_grammar() -> TestResult {
    let original = manifest(LOCK);
    for (name, pointer, replacement) in [
        ("source", "/source_commit", json!("b".repeat(40))),
        (
            "authority",
            "/workflow_authority_sha",
            json!("b".repeat(40)),
        ),
        (
            "signer",
            "/trusted_signing_identity/certificate_identity",
            json!("foreign"),
        ),
        (
            "repository",
            "/images/runner/repository",
            json!("ghcr.io/tailrocks/wrong"),
        ),
        ("tag", "/images/dind/tag", json!("latest")),
        ("platform", "/platform", json!("linux/arm64")),
        ("schema", "/schema_version", json!(1)),
        ("lock digest", "/runner_lock_sha256", json!("A".repeat(64))),
        ("wrong lock", "/runner_lock_sha256", json!("f".repeat(64))),
        (
            "config digest",
            "/images/runner/config_digest",
            json!(format!("sha256:{}", "z".repeat(64))),
        ),
        (
            "registry digest",
            "/images/runner/registry_digest",
            json!(format!("sha256:{}", "9".repeat(64))),
        ),
        (
            "build source",
            "/images/runner/build_source/source_commit",
            json!("b".repeat(40)),
        ),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).ok_or(name)? = replacement;
        reject(&serde_json::to_vec(&value)?, LOCK, name);
    }
    reject_missing_and_malformed_fields(&original)?;
    Ok(())
}

fn reject_missing_and_malformed_fields(original: &Value) -> TestResult {
    let mut missing = original.clone();
    missing["images"]
        .as_object_mut()
        .ok_or("images")?
        .remove("runner");
    reject(&serde_json::to_vec(&missing)?, LOCK, "missing role");
    let mut extra = original.clone();
    extra["images"]["extra"] = original["images"]["runner"].clone();
    reject(&serde_json::to_vec(&extra)?, LOCK, "extra role");
    let mut unknown = original.clone();
    unknown["unexpected"] = json!(true);
    reject(&serde_json::to_vec(&unknown)?, LOCK, "unknown field");
    assert!(
        runner_manifest::parse_for_source(
            &serde_json::to_vec(&original)?,
            "invalid",
            LOCK.as_bytes()
        )
        .is_err()
    );
    let missing_version = LOCK.replace("node_version = \"26.12.0\"\n", "");
    assert_ne!(missing_version, LOCK);
    reject(
        &serde_json::to_vec(&manifest(&missing_version))?,
        &missing_version,
        "missing lock version",
    );
    let mut missing_lock = original.clone();
    missing_lock
        .as_object_mut()
        .ok_or("manifest")?
        .remove("runner_lock_sha256");
    reject(&serde_json::to_vec(&missing_lock)?, LOCK, "missing lock");
    let serialized = serde_json::to_string(&original)?;
    let duplicate = serialized.replacen(
        "\"schema_version\":2",
        "\"schema_version\":2,\"schema_version\":2",
        1,
    );
    assert_ne!(serialized, duplicate);
    reject(duplicate.as_bytes(), LOCK, "duplicate field");
    Ok(())
}

#[test]
fn rejects_each_non_exact_lock_version() -> TestResult {
    for (field, version) in [
        ("runner_version", "2.338.0"),
        ("docker_version", "29.9.1"),
        ("buildx_version", "0.38.0"),
        ("compose_version", "5.6.0"),
        ("node_version", "26.12.0"),
    ] {
        for invalid in ["latest", "1.2.3-beta"] {
            let lock = LOCK.replace(
                &format!("{field} = \"{version}\""),
                &format!("{field} = \"{invalid}\""),
            );
            assert_ne!(lock, LOCK);
            let mut candidate = manifest(&lock);
            let role = field.strip_suffix("_version").ok_or("version field")?;
            candidate["versions"][role] = json!(invalid);
            reject(&serde_json::to_vec(&candidate)?, &lock, field);
        }
    }
    Ok(())
}
