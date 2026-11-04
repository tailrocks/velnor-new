// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::checker::{download_baseline, extract_baseline};
use super::*;
use crate::crate_release_registry::CrateRegistryResponse;

struct Registry {
    status: u16,
    body: Vec<u8>,
}
impl CrateRegistryRead for Registry {
    fn get(&self, _path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        Ok(CrateRegistryResponse {
            status: self.status,
            body: self.body.clone(),
        })
    }
}

#[test]
fn baseline_requires_authoritative_registry_and_uses_latest_previous_version()
-> Result<(), Box<dyn std::error::Error>> {
    let body = serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1,2,3]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false},{"id":2,"crate":"demo","num":"0.1.1","yanked":true},{"id":3,"crate":"demo","num":"0.2.0","yanked":false}]}).to_string().into_bytes();
    let registry = Registry { status: 200, body };
    assert_eq!(
        baseline(&registry, "demo", "0.1.2")?,
        Some(Baseline {
            version: "0.1.0".into(),
            id: 1
        })
    );
    assert!(baseline(&registry, "demo", "0.0.1").is_err());
    let yanked = serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":true}]}).to_string().into_bytes();
    assert!(
        baseline(
            &Registry {
                status: 200,
                body: yanked
            },
            "demo",
            "0.1.1"
        )
        .is_err()
    );
    let ambiguous = serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1,2]},"versions":[{"id":1,"crate":"demo","num":"0.1.0+one","yanked":false},{"id":2,"crate":"demo","num":"0.1.0+two","yanked":false}]}).to_string().into_bytes();
    assert!(
        baseline(
            &Registry {
                status: 200,
                body: ambiguous
            },
            "demo",
            "0.1.1"
        )
        .is_err()
    );
    assert_eq!(
        baseline(
            &Registry {
                status: 404,
                body: vec![]
            },
            "demo",
            "0.1.1"
        )?,
        None
    );
    for status in [403, 429, 500] {
        assert!(
            baseline(
                &Registry {
                    status,
                    body: vec![]
                },
                "demo",
                "0.1.1"
            )
            .is_err()
        );
    }
    assert!(
        baseline(
            &Registry {
                status: 200,
                body: b"{}".to_vec()
            },
            "demo",
            "0.1.1"
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn baseline_archive_is_bound_to_registry_identity_and_checksum()
-> Result<(), Box<dyn std::error::Error>> {
    struct ArchiveRegistry {
        detail: Vec<u8>,
        archive: Vec<u8>,
    }
    impl CrateRegistryRead for ArchiveRegistry {
        fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
            let body = if path == "/api/v1/crates/demo/0.1.0" {
                self.detail.clone()
            } else if path == "/crates/demo/demo-0.1.0.crate" {
                self.archive.clone()
            } else {
                return Err(invalid());
            };
            Ok(CrateRegistryResponse { status: 200, body })
        }
    }
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    let manifest = b"[package]\nname='demo'\nversion='0.1.0'\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "demo-0.1.0/Cargo.toml", manifest.as_slice())?;
    let archive = tar.into_inner()?.finish()?;
    let checksum = crate::generator::sha256_hex(&archive);
    let detail = serde_json::json!({"version":{"id":1,"crate":"demo","num":"0.1.0","yanked":false,"checksum":checksum}}).to_string().into_bytes();
    let mut registry = ArchiveRegistry { detail, archive };
    let previous = Baseline {
        version: "0.1.0".into(),
        id: 1,
    };
    let (bytes, observed) = download_baseline(&registry, "demo", &previous)?;
    assert_eq!(observed, checksum);
    let temp = tempfile::tempdir()?;
    extract_baseline(&bytes, "demo", "0.1.0", temp.path())?;
    assert_eq!(std::fs::read(temp.path().join("Cargo.toml"))?, manifest);
    let original = registry.detail.clone();
    for (field, value) in [
        ("id", serde_json::json!(2)),
        ("crate", serde_json::json!("other")),
        ("num", serde_json::json!("0.1.1")),
        ("yanked", serde_json::json!(true)),
        ("checksum", serde_json::json!("a".repeat(64))),
    ] {
        let mut detail: serde_json::Value = serde_json::from_slice(&original)?;
        detail["version"][field] = value;
        registry.detail = serde_json::to_vec(&detail)?;
        assert!(
            download_baseline(&registry, "demo", &previous).is_err(),
            "accepted {field}"
        );
    }
    registry.detail = format!(r#"{{"version":{{"id":1,"crate":"demo","num":"0.1.0","num":"0.1.1","yanked":false,"checksum":"{checksum}"}}}}"#).into_bytes();
    assert!(download_baseline(&registry, "demo", &previous).is_err());
    registry.detail = original;
    registry.archive.push(0);
    assert!(download_baseline(&registry, "demo", &previous).is_err());
    Ok(())
}
