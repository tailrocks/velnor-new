//! Secret-free host TOML. A `pat` or `token` field is rejected.

use serde::Deserialize;

use crate::error::HostError;

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
    /// `keychain:` reference.
    pub credential_ref: String,
}

/// Host limits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostLimits {
    /// Total permits. Defaults to 1.
    #[serde(default = "default_max_jobs")]
    pub max_jobs: u32,
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
        validate_github(&self.github)?;
        validate_docker(&self.docker)
    }
}

fn validate_github(github: &GithubSection) -> Result<(), HostError> {
    if repository_ok(&github.repository)
        && keychain_ref(&github.credential_ref)
        && github.scale_set_name == "ubuntu-26.04-scale-set"
    {
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

fn keychain_ref(value: &str) -> bool {
    let Some(name) = value.strip_prefix("keychain:") else {
        return false;
    };
    !name.is_empty() && !name.chars().any(char::is_whitespace)
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
