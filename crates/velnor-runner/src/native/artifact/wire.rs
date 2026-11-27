// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

fn int64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        Native(u64),
        Text(String),
    }
    match Number::deserialize(deserializer)? {
        Number::Native(number) => Ok(number),
        Number::Text(text) => text.parse().map_err(D::Error::custom),
    }
}

#[derive(Serialize)]
pub(super) struct Scope<'a> {
    pub(super) workflow_run_backend_id: &'a str,
    pub(super) workflow_job_run_backend_id: &'a str,
}

#[derive(Serialize)]
pub(super) struct CreateRequest<'a> {
    #[serde(flatten)]
    pub(super) scope: Scope<'a>,
    pub(super) name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) expires_at: Option<&'a str>,
    pub(super) version: u8,
    pub(super) mime_type: &'static str,
}

#[derive(Deserialize)]
pub(super) struct CreateResponse {
    #[serde(default)]
    pub(super) ok: bool,
    #[serde(alias = "signedUploadUrl")]
    #[serde(default)]
    pub(super) signed_upload_url: String,
}

#[derive(Serialize)]
pub(super) struct FinalizeRequest<'a> {
    #[serde(flatten)]
    pub(super) scope: Scope<'a>,
    pub(super) name: &'a str,
    pub(super) size: String,
    pub(super) hash: String,
}

#[derive(Deserialize)]
pub(super) struct FinalizeResponse {
    #[serde(default)]
    pub(super) ok: bool,
    #[serde(alias = "artifactId")]
    #[serde(default, deserialize_with = "int64")]
    pub(super) artifact_id: u64,
}

#[derive(Serialize)]
pub(super) struct ListRequest<'a> {
    #[serde(flatten)]
    pub(super) scope: Scope<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name_filter: Option<&'a str>,
}

#[derive(Deserialize)]
pub(super) struct ListResponse {
    #[serde(default)]
    pub(super) artifacts: Vec<ListedWire>,
}

#[derive(Deserialize)]
pub(super) struct ListedWire {
    #[serde(alias = "workflowRunBackendId")]
    pub(super) workflow_run_backend_id: String,
    #[serde(alias = "workflowJobRunBackendId")]
    pub(super) workflow_job_run_backend_id: String,
    #[serde(alias = "databaseId")]
    #[serde(deserialize_with = "int64")]
    pub(super) database_id: u64,
    pub(super) name: String,
    #[serde(default)]
    pub(super) digest: Option<String>,
}

#[derive(Serialize)]
pub(super) struct NamedRequest<'a> {
    #[serde(flatten)]
    pub(super) scope: Scope<'a>,
    pub(super) name: &'a str,
}

#[derive(Deserialize)]
pub(super) struct SignedUrlResponse {
    #[serde(alias = "signedUrl")]
    #[serde(default)]
    pub(super) signed_url: String,
}
