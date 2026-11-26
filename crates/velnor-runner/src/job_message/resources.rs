// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;

use super::wire::deserialize_null_default;
use crate::context_value::{ContextValue, ordinal_ignore_case_eq};
use crate::runner_json::ordered::OrderedJsonValue;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct JobResources {
    #[serde(deserialize_with = "deserialize_null_default", alias = "Endpoints")]
    pub endpoints: Vec<ServiceEndpoint>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Repositories")]
    pub repositories: Vec<Option<RepositoryResource>>,
    #[serde(deserialize_with = "deserialize_null_default", alias = "Containers")]
    pub containers: Vec<Option<ContainerResource>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEndpoint {
    #[serde(default, alias = "Name")]
    pub name: Option<String>,
    #[serde(default, alias = "Url")]
    pub url: Option<String>,
    #[serde(default, alias = "Authorization")]
    pub authorization: Option<EndpointAuthorization>,
    #[serde(default, deserialize_with = "deserialize_null_default", alias = "Data")]
    pub data: BTreeMap<String, Option<String>>,
}

impl ServiceEndpoint {
    #[must_use]
    pub fn data_string(&self, name: &str) -> Option<&str> {
        find_ignore_case(&self.data, name)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointAuthorization {
    #[serde(default, alias = "Scheme")]
    pub scheme: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_null_default",
        alias = "Parameters"
    )]
    pub parameters: BTreeMap<String, Option<String>>,
}

impl EndpointAuthorization {
    #[must_use]
    pub fn parameter_string(&self, name: &str) -> Option<&str> {
        find_ignore_case(&self.parameters, name)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryResource {
    #[serde(default, alias = "Alias")]
    pub alias: Option<String>,
    #[serde(default, alias = "Endpoint")]
    pub endpoint: Option<serde_json::Value>,
    #[serde(
        default = "empty_properties",
        deserialize_with = "deserialize_properties",
        alias = "Properties"
    )]
    pub properties: ContextValue,
}

impl RepositoryResource {
    #[must_use]
    pub fn property_str(&self, name: &str) -> Option<&str> {
        property_str(&self.properties, name)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerResource {
    #[serde(default, alias = "Alias")]
    pub alias: Option<String>,
    #[serde(default, alias = "Endpoint")]
    pub endpoint: Option<serde_json::Value>,
    #[serde(
        default = "empty_properties",
        deserialize_with = "deserialize_properties",
        alias = "Properties"
    )]
    pub properties: ContextValue,
}

impl ContainerResource {
    #[must_use]
    pub fn property_str(&self, name: &str) -> Option<&str> {
        property_str(&self.properties, name)
    }
}

fn find_ignore_case<'a>(map: &'a BTreeMap<String, Option<String>>, name: &str) -> Option<&'a str> {
    map.iter()
        .find(|(key, _)| ordinal_ignore_case_eq(key, name))
        .and_then(|(_, value)| value.as_deref())
}

fn property_str<'a>(properties: &'a ContextValue, name: &str) -> Option<&'a str> {
    match properties.get(name)? {
        ContextValue::String(value) => Some(value),
        _ => None,
    }
}

fn empty_properties() -> ContextValue {
    ContextValue::Object {
        case_sensitive: false,
        entries: Vec::new(),
    }
}

fn deserialize_properties<'de, D>(deserializer: D) -> Result<ContextValue, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<OrderedJsonValue>::deserialize(deserializer)? {
        None => Ok(empty_properties()),
        Some(value) => {
            ContextValue::from_ordered_property_bag(value).map_err(serde::de::Error::custom)
        }
    }
}
