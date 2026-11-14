// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Descriptor {
    #[serde(rename = "mediaType")]
    pub(super) media_type: String,
    pub(super) digest: String,
    pub(super) size: u64,
    #[serde(default)]
    pub(super) annotations: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(super) platform: Option<Platform>,
    #[serde(default, rename = "artifactType")]
    pub(super) artifact_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Platform {
    pub(super) architecture: String,
    pub(super) os: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OciManifest {
    #[serde(rename = "schemaVersion")]
    pub(super) schema_version: u8,
    #[serde(rename = "mediaType")]
    pub(super) media_type: String,
    pub(super) config: Descriptor,
    pub(super) layers: Vec<Descriptor>,
    #[serde(default, rename = "artifactType")]
    pub(super) artifact_type: Option<String>,
    #[serde(default)]
    pub(super) subject: Option<Value>,
}
