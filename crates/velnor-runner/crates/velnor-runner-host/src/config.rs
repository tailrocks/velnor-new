//! Secret-free host TOML. A `pat` or `token` field is rejected.

use std::fmt::{Display, Formatter};

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::HostError;
use crate::worker::{ResourceBudget, ResourceBudgetConfig};

mod connection;
mod release_read;
mod source_policy;

pub use connection::{ConnectionId, RepositoryConnection, ScaleSetSelector};
use release_read::{ReleaseReadConfig, validate_release_read};
pub use source_policy::{PullRequestRule, SourcePolicy, SourcePolicyMode, WorkflowEvent};

/// Top-level controller file. Schema 2 has one shared host budget.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    /// Must be 2.
    pub schema: u32,
    /// Repository bindings that share this host.
    connections: Vec<RepositoryConnection>,
    /// Optional host-level credential for verified release artifacts.
    #[serde(default)]
    release_read: Option<ReleaseReadConfig>,
    /// Capacity.
    pub host: HostLimits,
    /// Selected Docker context.
    pub docker: DockerConfig,
}

/// A parsed `keychain:<service>/<account>` reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeychainReference {
    service: String,
    account: String,
}

impl KeychainReference {
    /// Parse a canonical Keychain reference.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] for an empty or malformed pair.
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

    /// Keychain service name.
    #[must_use]
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Keychain account name.
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

/// Shared host limits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostLimits {
    /// Total permits. Defaults to 1.
    #[serde(default = "default_max_jobs")]
    pub max_jobs: u32,
    /// Required per-job runner and private `DinD` CPU and memory budgets.
    pub(crate) resources: ResourceBudgetConfig,
}

/// Docker binding.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DockerConfig {
    /// Context name. Not a provider switch.
    pub context: String,
    /// Requested platform. `linux/amd64` only.
    pub platform: String,
    /// `unix://` socket. `tcp://` and `ssh://` are rejected.
    pub endpoint: String,
}

const fn default_max_jobs() -> u32 {
    1
}

impl HostConfig {
    /// Parse and validate.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] for schema, secret fields, or endpoints.
    pub fn parse(text: &str) -> Result<Self, HostError> {
        let mut config: Self = toml::from_str(text).map_err(|_| HostError::Config)?;
        for connection in &mut config.connections {
            connection.canonicalize()?;
        }
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
        validate_release_read(
            self.release_read.as_ref(),
            self.connections
                .iter()
                .map(|connection| &connection.credential_ref),
        )?;
        validate_docker(&self.docker)
    }

    /// Validated container and aggregate limits from required host configuration.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when a resource limit is zero or overflows.
    pub fn resource_budget(&self) -> Result<ResourceBudget, HostError> {
        self.host.resources.validate()
    }

    /// All repository connections that share this host.
    #[must_use]
    pub fn connections(&self) -> &[RepositoryConnection] {
        &self.connections
    }

    /// Host-level Keychain reference for release metadata and attestations.
    #[must_use]
    pub fn release_read_credential_ref(&self) -> Option<&KeychainReference> {
        self.release_read
            .as_ref()
            .map(ReleaseReadConfig::credential_ref)
    }

    /// Set the separate host-level release-reader Keychain reference.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] if it reuses a repository credential.
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

    /// Shared host job limit.
    #[must_use]
    pub(crate) const fn max_jobs(&self) -> u32 {
        self.host.max_jobs
    }

    /// Selected Docker endpoint and platform.
    #[must_use]
    pub(crate) const fn docker(&self) -> &DockerConfig {
        &self.docker
    }

    /// Find one connection by its stable ID.
    #[must_use]
    pub fn connection(&self, id: &ConnectionId) -> Option<&RepositoryConnection> {
        self.connections
            .iter()
            .find(|connection| &connection.connection_id == id)
    }

    /// Add one connection without changing an existing binding.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when any host or connection rule fails.
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

    /// Remove one repository binding and keep the shared host settings.
    ///
    /// An empty connection list is a disconnected state. Callers must not start
    /// repository listeners until the list contains a connection.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when the current config is invalid.
    pub fn remove_connection(&mut self, id: &ConnectionId) -> Result<bool, HostError> {
        self.validate()?;
        let Some(index) = self
            .connections
            .iter()
            .position(|connection| &connection.connection_id == id)
        else {
            return Ok(false);
        };
        self.connections.remove(index);
        self.validate()?;
        Ok(true)
    }

    /// Serialize the complete, validated host configuration.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when a config rule fails.
    pub fn to_toml(&self) -> Result<String, HostError> {
        self.validate()?;
        let document = HostToml {
            schema: self.schema,
            connections: &self.connections,
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

fn validate_docker(docker: &DockerConfig) -> Result<(), HostError> {
    if docker.platform == "linux/amd64"
        && !docker.context.is_empty()
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
