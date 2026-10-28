// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DescriptorDocument {
    pub(crate) schema_version: u8,
    pub(crate) runner_lock_sha256: String,
    pub(crate) source_commit: String,
    pub(crate) platform: String,
    pub(crate) images: ImageEntries,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImageEntries {
    pub(crate) runner: ImageEntry,
    pub(crate) dind: ImageEntry,
    pub(crate) resource_probe: ImageEntry,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImageEntry {
    pub(crate) repository: String,
    pub(crate) tag: String,
    pub(crate) registry_digest: String,
    pub(crate) platform_manifest_digest: String,
    pub(crate) config_digest: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Versions {
    pub(crate) runner: String,
    pub(crate) docker: String,
    pub(crate) buildx: String,
    pub(crate) compose: String,
    pub(crate) node: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrustedSigningIdentity {
    pub(crate) oidc_issuer: String,
    pub(crate) certificate_identity: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceProbeProfile {
    pub(crate) protocol_version: u8,
    pub(crate) image_user: String,
    pub(crate) entrypoint: String,
    pub(crate) working_directory: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunnerImageManifest {
    pub(crate) schema_version: u8,
    pub(crate) repository: String,
    pub(crate) source_ref: String,
    pub(crate) source_commit: String,
    pub(crate) signer_workflow: String,
    pub(crate) signer_ref: String,
    pub(crate) workflow_authority_sha: String,
    pub(crate) trusted_signing_identity: TrustedSigningIdentity,
    pub(crate) platform: String,
    pub(crate) versions: Versions,
    pub(crate) images: ImageEntries,
    pub(crate) resource_probe: ResourceProbeProfile,
}
