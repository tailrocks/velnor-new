// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use serde::Deserialize;
use serde_json::Value;

use super::wire::{deserialize_guid, empty_guid};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TaskOrchestrationPlanReference {
    #[serde(alias = "ScopeIdentifier")]
    pub scope_identifier: Option<String>,
    #[serde(alias = "PlanType")]
    pub plan_type: Option<String>,
    #[serde(alias = "Version")]
    pub version: Option<i32>,
    #[serde(
        default = "empty_guid",
        deserialize_with = "deserialize_guid",
        alias = "PlanId"
    )]
    pub plan_id: String,
    #[serde(alias = "PlanGroup")]
    pub plan_group: Option<String>,
    #[serde(alias = "ArtifactUri")]
    pub artifact_uri: Option<String>,
    #[serde(alias = "ArtifactLocation")]
    pub artifact_location: Option<String>,
    #[serde(alias = "Definition")]
    pub definition: Option<Value>,
    #[serde(alias = "Owner")]
    pub owner: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TimelineReference {
    #[serde(
        default = "empty_guid",
        deserialize_with = "deserialize_guid",
        alias = "Id"
    )]
    pub id: String,
    #[serde(alias = "ChangeId")]
    pub change_id: Option<i32>,
    #[serde(alias = "Location")]
    pub location: Option<String>,
}
