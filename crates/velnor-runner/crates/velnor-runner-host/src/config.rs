// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter};

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::HostError;
use crate::worker::{ResourceBudget, ResourceBudgetConfig};

mod connection;
mod job_authority;
mod platform;
mod release_read;
mod source_policy;

pub use connection::{ConnectionId, RepositoryConnection, ScaleSetSelector};
pub(crate) use job_authority::{
    ApprovedJobAuthorityEntry, ApprovedWorkflowSource, JobAuthorityLookup,
};
pub use job_authority::{
    ApprovedJobAuthorityPolicy, JobAuthorityRole, build_job_authority_policy_json,
};
pub use platform::RunnerPlatform;
use release_read::{ReleaseReadConfig, validate_release_read};
pub use source_policy::{PullRequestRule, SourcePolicy, SourcePolicyMode, WorkflowEvent};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    pub schema: u32,

    connections: Vec<RepositoryConnection>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    job_authority_policies: Vec<ApprovedJobAuthorityPolicy>,

    #[serde(default)]
    release_read: Option<ReleaseReadConfig>,

    pub host: HostLimits,

    pub docker: DockerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeychainReference {
    service: String,
    account: String,
}

impl KeychainReference {
    pub fn parse(value: &str) -> Result<Self, HostError> {
        let Some(pair) = value.strip_prefix("keychain:") else {
            return Err(HostError::Config);
        };
        let Some((service, account)) = pair.split_once('/') else {
            return Err(HostError::Config);
        };
        if !valid_keychain_component(service)
            || !valid_keychain_component(account)
            || account.contains('/')
        {
            return Err(HostError::Config);
        }
        Ok(Self {
            service: service.to_owned(),
            account: account.to_owned(),
        })
    }

    #[must_use]
    pub fn service(&self) -> &str {
        &self.service
    }

    #[must_use]
    pub fn account(&self) -> &str {
        &self.account
    }
}

impl Display for KeychainReference {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "keychain:{}/{}", self.service, self.account)
    }
}

impl Serialize for KeychainReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for KeychainReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

fn valid_keychain_component(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|character| {
            character.is_whitespace() || character.is_control() || matches!(character, '/' | '\\')
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostLimits {
    #[serde(default = "default_max_jobs")]
    pub max_jobs: u32,

    pub(crate) resources: ResourceBudgetConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DockerConfig {
    pub context: String,

    pub platform: String,

    pub endpoint: String,
}

const fn default_max_jobs() -> u32 {
    1
}

impl HostConfig {
    pub fn parse(text: &str) -> Result<Self, HostError> {
        let mut config: Self = toml::from_str(text).map_err(|_| HostError::Config)?;
        for connection in &mut config.connections {
            connection.canonicalize()?;
        }
        for policy in &mut config.job_authority_policies {
            policy.canonicalize();
        }
        config
            .job_authority_policies
            .sort_by(|left, right| left.connection_id().cmp(right.connection_id()));
        config.validate()?;
        Ok(config)
    }

    pub(crate) fn validate(&self) -> Result<(), HostError> {
        if self.schema != 2 || self.host.max_jobs == 0 {
            return Err(HostError::Config);
        }
        self.resource_budget()?
            .pair()
            .docker_limits(self.host.max_jobs)?;
        validate_connections(&self.connections)?;
        job_authority::validate_policies(&self.job_authority_policies, &self.connections)?;
        validate_release_read(
            self.release_read.as_ref(),
            self.connections
                .iter()
                .map(|connection| &connection.credential_ref),
        )?;
        validate_docker(&self.docker, &self.connections)
    }

    pub fn resource_budget(&self) -> Result<ResourceBudget, HostError> {
        self.host.resources.validate()
    }

    #[must_use]
    pub fn connections(&self) -> &[RepositoryConnection] {
        &self.connections
    }

    #[must_use]
    pub fn release_read_credential_ref(&self) -> Option<&KeychainReference> {
        self.release_read
            .as_ref()
            .map(ReleaseReadConfig::credential_ref)
    }

    pub fn set_release_read_credential_ref(
        &mut self,
        credential_ref: KeychainReference,
    ) -> Result<(), HostError> {
        let previous = self.release_read.clone();
        self.release_read = Some(ReleaseReadConfig::new(credential_ref));
        if let Err(error) = self.validate() {
            self.release_read = previous;
            return Err(error);
        }
        Ok(())
    }

    #[must_use]
    pub(crate) const fn max_jobs(&self) -> u32 {
        self.host.max_jobs
    }

    #[must_use]
    pub(crate) const fn docker(&self) -> &DockerConfig {
        &self.docker
    }

    #[must_use]
    pub fn connection(&self, id: &ConnectionId) -> Option<&RepositoryConnection> {
        self.connections
            .iter()
            .find(|connection| &connection.connection_id == id)
    }

    pub(crate) fn validated_job_authority_policy(
        &self,
        id: &ConnectionId,
    ) -> Result<Option<&ApprovedJobAuthorityPolicy>, HostError> {
        self.validate()?;
        Ok(self
            .job_authority_policies
            .iter()
            .find(|policy| policy.connection_id() == id))
    }

    pub fn add_connection(
        &mut self,
        mut connection: RepositoryConnection,
    ) -> Result<(), HostError> {
        self.validate()?;
        connection.canonicalize()?;
        let previous_len = self.connections.len();
        self.connections.push(connection);
        if let Err(error) = self.validate() {
            self.connections.truncate(previous_len);
            return Err(error);
        }
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String, HostError> {
        self.validate()?;
        let document = HostToml {
            schema: self.schema,
            connections: &self.connections,
            job_authority_policies: &self.job_authority_policies,
            release_read: self.release_read.as_ref(),
            host: HostTomlLimits {
                max_jobs: self.host.max_jobs,
                resources: HostTomlResources {
                    runner_cpu_millicores: self.host.resources.runner_cpu_millicores,
                    runner_memory_bytes: self.host.resources.runner_memory_bytes,
                    dind_cpu_millicores: self.host.resources.dind_cpu_millicores,
                    dind_memory_bytes: self.host.resources.dind_memory_bytes,
                    operational_memory_reserve_bytes: self
                        .host
                        .resources
                        .operational_memory_reserve_bytes,
                },
            },
            docker: &self.docker,
        };
        toml::to_string(&document).map_err(|_| HostError::Config)
    }
}

#[derive(Serialize)]
struct HostToml<'a> {
    schema: u32,
    connections: &'a [RepositoryConnection],
    job_authority_policies: &'a [ApprovedJobAuthorityPolicy],
    #[serde(skip_serializing_if = "Option::is_none")]
    release_read: Option<&'a ReleaseReadConfig>,
    host: HostTomlLimits,
    docker: &'a DockerConfig,
}

#[derive(Serialize)]
struct HostTomlLimits {
    max_jobs: u32,
    resources: HostTomlResources,
}

#[derive(Serialize)]
struct HostTomlResources {
    runner_cpu_millicores: u64,
    runner_memory_bytes: u64,
    dind_cpu_millicores: u64,
    dind_memory_bytes: u64,
    operational_memory_reserve_bytes: u64,
}

fn validate_docker(
    docker: &DockerConfig,
    connections: &[RepositoryConnection],
) -> Result<(), HostError> {
    if RunnerPlatform::parse(&docker.platform).is_some_and(|platform| {
        connections
            .iter()
            .all(|connection| platform.routes(&connection.scale_set))
    }) && !docker.context.is_empty()
        && unix_endpoint(&docker.endpoint)
    {
        Ok(())
    } else {
        Err(HostError::Config)
    }
}

fn validate_connections(connections: &[RepositoryConnection]) -> Result<(), HostError> {
    let mut ids = std::collections::HashSet::new();
    let mut repository_ids = std::collections::HashSet::new();
    let mut repositories = std::collections::HashSet::new();
    let mut credential_refs = std::collections::HashSet::new();
    let mut scale_set_names = std::collections::HashSet::new();
    for connection in connections {
        connection.validate()?;
        if !ids.insert(connection.connection_id.clone())
            || !repository_ids.insert(connection.repository_id)
            || !repositories.insert(connection.repository.to_ascii_lowercase())
            || !credential_refs.insert(connection.credential_ref.clone())
            || !scale_set_names.insert(connection.scale_set.name().to_owned())
        {
            return Err(HostError::Config);
        }
    }
    for connection in connections {
        if connections.iter().any(|other| {
            other.connection_id != connection.connection_id
                && other
                    .scale_set
                    .labels()
                    .iter()
                    .any(|label| label == connection.scale_set.name())
        }) {
            return Err(HostError::Config);
        }
    }
    Ok(())
}

fn unix_endpoint(endpoint: &str) -> bool {
    let Some(path) = endpoint.strip_prefix("unix://") else {
        return false;
    };
    path.starts_with('/')
        && path.len() > 1
        && !path.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}
