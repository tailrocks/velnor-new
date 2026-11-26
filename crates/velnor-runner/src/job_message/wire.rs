// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::BTreeMap;

use super::context_data::pipeline_context_value;
use super::ordered_context::OrderedContextData;
use super::plan::{TaskOrchestrationPlanReference, TimelineReference};
use super::resources::{JobResources, ServiceEndpoint};
use super::step::ActionStep;
use super::variables::{MaskHint, VariableValue};
use crate::context_value::{ContextValue, ordinal_ignore_case_eq};
use crate::secret::Secret;

pub const PIPELINE_AGENT_JOB_REQUEST: &str = "PipelineAgentJobRequest";

const SYSTEM_CONNECTION: &str = "SystemVssConnection";

pub(super) fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

pub(super) fn empty_guid() -> String {
    "00000000-0000-0000-0000-000000000000".to_owned()
}

pub(super) fn deserialize_guid<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_else(empty_guid))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentJobRequestMessage {
    #[serde(deserialize_with = "deserialize_null_default", alias = "MessageType")]
    pub message_type: String,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Plan")]
    pub plan: TaskOrchestrationPlanReference,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Timeline")]
    pub timeline: TimelineReference,
    #[serde(
        default = "empty_guid",
        deserialize_with = "deserialize_guid",
        alias = "JobId"
    )]
    pub job_id: String,
    #[serde(
        deserialize_with = "deserialize_null_default",
        alias = "JobDisplayName"
    )]
    pub job_display_name: String,
    #[serde(alias = "JobName")]
    pub job_name: Option<String>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "RequestId")]
    pub request_id: i64,
    #[serde(alias = "LockedUntil")]
    pub locked_until: Option<String>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Variables")]
    pub variables: BTreeMap<String, VariableValue>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Mask")]
    pub mask: Vec<MaskHint>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Resources")]
    pub resources: JobResources,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Steps")]
    pub steps: Vec<ActionStep>,
    #[serde(
        deserialize_with = "deserialize_null_default",
        alias = "EnvironmentVariables"
    )]
    pub environment_variables: Vec<Value>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Defaults")]
    pub defaults: Vec<Value>,
    #[serde(alias = "JobContainer")]
    pub job_container: Option<Value>,
    #[serde(alias = "JobServiceContainers")]
    pub job_service_containers: Option<Value>,
    #[serde(alias = "JobSidecarContainers")]
    pub job_sidecar_containers: Option<BTreeMap<String, Option<String>>>,
    #[serde(alias = "JobOutputs")]
    pub job_outputs: Option<Value>,
    #[serde(alias = "Workspace")]
    pub workspace: Option<Value>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "ContextData")]
    pub context_data: Secret<OrderedContextData<Value>>,
    #[serde(alias = "ActionsEnvironment")]
    pub actions_environment: Option<Value>,
    #[serde(alias = "BillingOwnerId")]
    pub billing_owner_id: Option<String>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "dependencies")]
    pub actions_dependencies: Vec<Option<String>>,
}

impl AgentJobRequestMessage {
    pub fn parse_json(body: &str) -> Result<Self> {
        serde_json::from_str(body).context("parse AgentJobRequestMessage")
    }

    pub fn from_value(value: Value) -> Result<Self> {
        serde_json::from_value(value).context("parse AgentJobRequestMessage value")
    }

    pub fn materialize_context_values(&self) -> Result<Secret<BTreeMap<String, ContextValue>>> {
        Ok(Secret::new(
            self.materialize_context_values_ordered()?
                .into_inner()
                .into_iter()
                .collect(),
        ))
    }

    pub fn materialize_context_values_ordered(
        &self,
    ) -> Result<Secret<Vec<(String, ContextValue)>>> {
        self.context_data
            .expose()
            .iter()
            .map(|(name, value)| Ok((name.clone(), pipeline_context_value(value)?)))
            .collect::<Result<_>>()
            .map(Secret::new)
    }

    pub fn system_connection(&self) -> Result<&ServiceEndpoint> {
        let mut matches = self.resources.endpoints.iter().filter(|endpoint| {
            endpoint
                .name
                .as_deref()
                .is_some_and(|name| ordinal_ignore_case_eq(name, SYSTEM_CONNECTION))
        });
        let endpoint = matches
            .next()
            .context("missing SystemVssConnection endpoint")?;
        if matches.next().is_some() {
            bail!("multiple SystemVssConnection endpoints");
        }
        Ok(endpoint)
    }
}
