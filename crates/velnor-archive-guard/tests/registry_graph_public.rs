// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod family_oci_deadline;

use std::{error::Error, io::Write};

use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct ImageGraph {
    manifest: Vec<u8>,
    manifest_digest: String,
    config: Vec<u8>,
    config_digest: String,
    layers: Vec<Vec<u8>>,
    diff_ids: Vec<String>,
    runtime_config: Value,
}

impl ImageGraph {
    fn new(source: &str, role: &str) -> Result<Self> {
        let (user, entrypoint, working_dir) = match role {
            "runner" => ("runner", "/usr/local/bin/velnor-runner-entrypoint", ""),
            "dind" => ("", "/usr/local/bin/velnor-dind-entrypoint", ""),
            "resource-probe" => ("65532:65532", "/velnor/resource-probe", "/"),
            _ => return Err("unknown image role".into()),
        };
        let mut layers = Vec::new();
        let mut diff_ids = Vec::new();
        for index in 0..2 {
            let mut tar = tar::Builder::new(Vec::new());
            let contents = format!("{role} layer {index}\n");
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, format!("layer-{index}"), contents.as_bytes())?;
            let raw = tar.into_inner()?;
            diff_ids.push(digest(&raw));
            let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
            gzip.write_all(&raw)?;
            layers.push(gzip.finish()?);
        }
        let runtime_config = json!({
            "User": user,
            "WorkingDir": working_dir,
            "Entrypoint": [entrypoint],
            "Labels": {
                "org.opencontainers.image.revision": source,
                "io.velnor.image.kind": role
            }
        });
        let config = serde_json::to_vec(&json!({
            "architecture": "amd64",
            "os": "linux",
            "config": runtime_config,
            "rootfs": {"type": "layers", "diff_ids": diff_ids}
        }))?;
        let config_digest = digest(&config);
        let layer_descriptors: Vec<_> = layers
            .iter()
            .map(|layer| {
                json!({
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "digest": digest(layer),
                    "size": layer.len()
                })
            })
            .collect();
        let manifest = serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": config_digest,
                "size": config.len()
            },
            "layers": layer_descriptors
        }))?;
        let manifest_digest = digest(&manifest);
        Ok(Self {
            manifest,
            manifest_digest,
            config,
            config_digest,
            layers,
            diff_ids,
            runtime_config,
        })
    }
}

fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 15)]));
    }
    format!("sha256:{hex}")
}

#[test]
fn authenticated_registry_graph_preserves_order_and_role_configuration() -> Result<()> {
    for role in ["runner", "dind", "resource-probe"] {
        let graph = ImageGraph::new(&"a".repeat(40), role)?;
        let manifest =
            velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_manifest(
                &graph.manifest,
                &graph.manifest_digest,
            )?;
        assert_eq!(manifest.manifest_size(), graph.manifest.len() as u64);
        assert_eq!(manifest.config_digest(), graph.config_digest);
        let result =
            velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_configuration(manifest, &graph.config)?;
        assert_eq!(result.diff_ids, graph.diff_ids);
        assert_eq!(result.diff_ids.len(), graph.layers.len());
        assert_eq!(
            result.configuration.user.as_deref(),
            graph.runtime_config["User"].as_str()
        );
        assert_eq!(
            serde_json::to_value(&result.configuration.entrypoint)?,
            graph.runtime_config["Entrypoint"]
        );
        assert_eq!(
            result.configuration.working_directory.as_deref(),
            graph.runtime_config["WorkingDir"].as_str()
        );
        assert_eq!(
            serde_json::to_value(&result.configuration.labels)?,
            graph.runtime_config["Labels"]
        );
        assert_eq!(result.manifest.manifest_digest(), graph.manifest_digest);
    }
    Ok(())
}

#[test]
fn changed_registry_bytes_and_unauthenticated_configuration_are_rejected() -> Result<()> {
    let graph = ImageGraph::new(&"b".repeat(40), "runner")?;
    let mut wrong_size: Value = serde_json::from_slice(&graph.manifest)?;
    wrong_size["config"]["size"] = json!(graph.config.len() + 1);
    let encoded = serde_json::to_vec(&wrong_size)?;
    let declared_size =
        velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_manifest(
            &encoded,
            &digest(&encoded),
        )?;
    assert!(
        velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_configuration(
            declared_size,
            &graph.config
        )
        .is_err(),
        "accepted configuration with an authenticated incorrect declared size"
    );
    let mut changed_manifest = graph.manifest.clone();
    changed_manifest.push(b' ');
    assert!(
        velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_manifest(
            &changed_manifest,
            &graph.manifest_digest,
        )
        .is_err()
    );
    assert!(
        family_oci_deadline::expire_during(|| {
            velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_manifest(
                &graph.manifest,
                &graph.manifest_digest,
            )
        })?
        .is_err()
    );
    let manifest =
        velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_manifest(
            &graph.manifest,
            &graph.manifest_digest,
        )?;
    let mut changed_config = graph.config.clone();
    let offset = changed_config
        .windows(40)
        .position(|window| window == b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        .ok_or("revision missing")?;
    changed_config[offset] = b'c';
    assert!(
        velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_configuration(
            manifest.clone(),
            &changed_config,
        )
        .is_err()
    );
    let mut resized_config = graph.config.clone();
    resized_config.push(b' ');
    assert!(
        velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_configuration(
            manifest.clone(),
            &resized_config,
        )
        .is_err()
    );
    assert!(
        family_oci_deadline::expire_during(|| velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_configuration(manifest, &graph.config))?
            .is_err()
    );
    Ok(())
}

#[test]
fn authenticated_but_invalid_manifest_and_config_are_rejected() -> Result<()> {
    let graph = ImageGraph::new(&"c".repeat(40), "runner")?;
    let original_manifest: Value = serde_json::from_slice(&graph.manifest)?;
    let original_config: Value = serde_json::from_slice(&graph.config)?;
    for (field, value) in [
        ("mediaType", json!("application/unknown")),
        ("schemaVersion", json!(1)),
    ] {
        let mut invalid = original_manifest.clone();
        invalid[field] = value;
        let bytes = serde_json::to_vec(&invalid)?;
        assert!(
            velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_manifest(
                &bytes,
                &digest(&bytes)
            )
            .is_err()
        );
    }
    for config in [
        {
            let mut value = original_config.clone();
            value["architecture"] = json!("arm64");
            value
        },
        {
            let mut value = original_config.clone();
            value["rootfs"]["diff_ids"] = json!([graph.diff_ids[0]]);
            value
        },
        {
            let mut value = original_config.clone();
            value["rootfs"]["diff_ids"][0] = json!("sha256:wrong");
            value
        },
    ] {
        let bytes = serde_json::to_vec(&config)?;
        let mut manifest = original_manifest.clone();
        manifest["config"]["digest"] = json!(digest(&bytes));
        manifest["config"]["size"] = json!(bytes.len());
        let encoded = serde_json::to_vec(&manifest)?;
        let parsed =
            velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_manifest(
                &encoded,
                &digest(&encoded),
            )?;
        assert!(
            velnor_archive_guard::owned_archive_guard::oci_candidate::inspect_registry_configuration(parsed, &bytes).is_err()
        );
    }
    Ok(())
}
