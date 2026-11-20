// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateWorkflow {
    pub repository: String,

    pub repository_id: u64,

    pub event: String,

    pub workflow_ref: String,

    pub authority_commit: String,

    pub run_id: u64,

    pub run_attempt: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateArchive {
    pub sha256: String,

    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateOci {
    pub archive_index_digest: String,

    pub archive_index_size_bytes: u64,

    pub platform_manifest_digest: String,

    pub platform_manifest_size_bytes: u64,

    pub image_configuration_digest: String,

    pub image_configuration_size_bytes: u64,

    pub layer_digests: Vec<String>,
}
