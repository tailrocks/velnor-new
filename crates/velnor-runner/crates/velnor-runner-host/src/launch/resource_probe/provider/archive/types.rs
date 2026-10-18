use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OciIndex {
    #[serde(rename = "schemaVersion")]
    pub(super) schema_version: u8,
    #[serde(rename = "mediaType")]
    pub(super) media_type: String,
    pub(super) manifests: Vec<Descriptor>,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, PartialEq, Eq, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LegacyManifest {
    #[serde(rename = "Config")]
    pub(super) config: String,
    #[serde(rename = "RepoTags")]
    pub(super) repo_tags: Vec<String>,
    #[serde(rename = "Layers")]
    pub(super) layers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImageConfigFile {
    pub(super) architecture: String,
    pub(super) os: String,
    #[serde(default)]
    pub(super) variant: Option<String>,
    pub(super) config: Value,
    pub(super) rootfs: RootFs,
    #[serde(default, rename = "created")]
    _created: Option<String>,
    #[serde(default, rename = "history")]
    _history: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RootFs {
    #[serde(rename = "type")]
    pub(super) kind: String,
    #[serde(rename = "diff_ids")]
    pub(super) diff_ids: Vec<String>,
}
