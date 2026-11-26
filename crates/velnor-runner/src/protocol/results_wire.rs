// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Serialize)]
pub(super) struct ScopeRequest<'a> {
    #[serde(rename = "workflow_run_backend_id")]
    pub(super) run: &'a str,
    #[serde(rename = "workflow_job_run_backend_id")]
    pub(super) job: &'a str,
    #[serde(rename = "step_backend_id", skip_serializing_if = "Option::is_none")]
    pub(super) step: Option<&'a str>,
}

#[derive(Serialize)]
pub(super) struct MetadataRequest<'a> {
    #[serde(flatten)]
    pub(super) scope: &'a ScopeRequest<'a>,
    pub(super) uploaded_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) line_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) size: Option<i64>,
}

#[derive(Clone, Copy)]
pub(super) enum Measure {
    Lines(i64),
    Bytes,
}

#[derive(Deserialize)]
pub(super) struct SignedUrlResponse {
    pub(super) logs_url: Option<String>,
    pub(super) summary_url: Option<String>,
    pub(super) blob_storage_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_int64")]
    pub(super) soft_size_limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireInt64 {
    Number(i64),
    Text(String),
}

fn deserialize_int64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<WireInt64>::deserialize(deserializer)?
        .map(|value| match value {
            WireInt64::Number(number) => Ok(number),
            WireInt64::Text(text) => text.trim().parse().map_err(serde::de::Error::custom),
        })
        .transpose()
}

#[derive(Debug, thiserror::Error)]
#[error("{label} is {size} bytes, over the {limit} byte limit of its upload URL")]
pub(super) struct SoftSizeLimitExceeded {
    pub(super) label: &'static str,
    pub(super) size: i64,
    pub(super) limit: i64,
}

#[derive(Deserialize)]
pub(super) struct Acknowledgement {
    #[serde(default)]
    pub(super) ok: bool,
}
