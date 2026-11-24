// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Map, Value, json};

use super::checks::CiRun;
use super::gate::Identity;
use super::location::{PACKAGE, REGISTRY};
use crate::error::CiError;
use crate::image_candidate::canonical;

pub const SCHEMA: &str = "velnor.job-image-publication.v1";
pub const MAX_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub struct Published<'a> {
    pub record: &'a Map<String, Value>,
    pub record_sha256: &'a str,
    pub identity: &'a Identity,
    pub ci: CiRun,
    pub oci: &'a Value,
}

pub fn encode(published: &Published<'_>) -> Result<Vec<u8>, CiError> {
    let Published {
        record,
        record_sha256,
        identity,
        ci,
        oci,
    } = published;
    let manifest = oci["platform_manifest_digest"].as_str().unwrap_or_default();
    let evidence = json!({
        "schema": SCHEMA,
        "status": "PASS",
        "package": format!("{REGISTRY}/{PACKAGE}"),
        "digest_reference": format!("{REGISTRY}/{PACKAGE}@{manifest}"),
        "private_package_access_gate": "not_proven",
        "archive": {
            "sha256": record["archive"]["sha256"],
            "size_bytes": record["archive"]["size_bytes"],
        },
        "source": identity.source,
        "workflow": {
            "ref": identity.workflow_ref,
            "authority_sha": identity.source,
            "run_id": identity.run_id,
            "run_attempt": identity.run_attempt,
            "artifact_id": identity.artifact_id,
        },
        "ci": {"run_id": ci.run_id, "run_attempt": ci.run_attempt},
        "oci": {
            "platform": "linux/amd64",
            "archive_index_digest": format!(
                "sha256:{}",
                oci["archive_index_digest"].as_str().unwrap_or_default()
            ),
            "manifest_digest": manifest,
            "manifest_size_bytes": oci["platform_manifest_size_bytes"],
            "config_digest": oci["image_configuration_digest"],
            "layer_digests": oci["layer_digests"],
        },
        "record_sha256": record_sha256,
    });
    let bytes = canonical::encode(&evidence);
    if bytes.len() > MAX_BYTES {
        return Err(CiError::new("Publication evidence exceeds its size limit."));
    }
    Ok(bytes)
}
