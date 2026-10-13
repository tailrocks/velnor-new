//! Secret-free host TOML. A `pat` or `token` field is rejected.

use std::fmt::{Display, Formatter};

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};

use crate::error::HostError;
use crate::worker::{ResourceBudget, ResourceBudgetConfig};

/// Top-level controller file. Schema 1. Unknown fields fail.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    /// Must be 1.
    pub schema: u32,
    /// Repository binding.
    pub github: GithubSection,
    /// Capacity.
    pub host: HostLimits,
    /// Selected Docker context.
    pub docker: DockerConfig,
}

/// GitHub binding. Credentials are a Keychain reference, never a token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubSection {
    /// `owner/name`.
    pub repository: String,
    /// Scale set name.
    pub scale_set_name: String,
    /// Keychain service and account used by every host credential operation.
    pub credential_ref: KeychainReference,
}

/// A parsed `keychain:<service>/<account>` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
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
            character.is_whitespace() || character.is_control() || character == '/'
        })
}

/// Host limits.
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
        let config: Self = toml::from_str(text).map_err(|_| HostError::Config)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), HostError> {
        if self.schema != 1 || self.host.max_jobs == 0 {
            return Err(HostError::Config);
        }
        self.resource_budget()?
            .pair()
            .docker_limits(self.host.max_jobs)?;
        validate_github(&self.github)?;
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
}

fn validate_github(github: &GithubSection) -> Result<(), HostError> {
    if repository_ok(&github.repository) && github.scale_set_name == "ubuntu-26.04-scale-set" {
        Ok(())
    } else {
        Err(HostError::Config)
    }
}

fn repository_ok(repository: &str) -> bool {
    let mut parts = repository.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(name), None) => {
            !owner.is_empty() && !name.is_empty() && !repository.contains(' ')
        }
        _ => false,
    }
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

fn unix_endpoint(endpoint: &str) -> bool {
    let Some(path) = endpoint.strip_prefix("unix://") else {
        return false;
    };
    path.starts_with('/')
        && path.len() > 1
        && !path.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}
