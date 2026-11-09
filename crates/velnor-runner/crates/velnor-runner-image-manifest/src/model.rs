// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageEntries {
    pub runner: ImageEntry,
    pub dind: ImageEntry,
    pub resource_probe: ImageEntry,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageEntry {
    pub repository: String,
    pub tag: String,
    pub registry_digest: String,
    pub platform_manifest_digest: String,
    pub config_digest: String,
    pub build_source: BuildSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSource {
    pub metadata_sha256: String,
    pub build_type: String,
    pub source_repository: String,
    pub source_commit: String,
    pub context_path: String,
    pub dockerfile_path: String,
    pub builder_platform: String,
    pub named_contexts: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Versions {
    pub runner: String,
    pub docker: String,
    pub buildx: String,
    pub compose: String,
    pub node: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedSigningIdentity {
    pub oidc_issuer: String,
    pub certificate_identity: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceProbeProfile {
    pub protocol_version: u8,
    pub image_user: String,
    pub entrypoint: String,
    pub working_directory: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImageManifest {
    pub schema_version: u8,
    pub repository: String,
    pub source_ref: String,
    pub source_commit: String,
    pub runner_lock_sha256: String,
    pub signer_workflow: String,
    pub signer_ref: String,
    pub workflow_authority_sha: String,
    pub trusted_signing_identity: TrustedSigningIdentity,
    pub platform: String,
    pub versions: Versions,
    pub images: ImageEntries,
    pub resource_probe: ResourceProbeProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageContractError {
    CanonicalJson(String),
    DocumentTooLarge { size: usize, limit: usize },
    DocumentTooDeep { depth: usize, limit: usize },
}
