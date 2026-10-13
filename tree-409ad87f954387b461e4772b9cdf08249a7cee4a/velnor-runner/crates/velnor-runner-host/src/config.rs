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
    let repo_ok = github.repository.split('/').count() == 2 && !github.repository.contains(' ');
    let ref_ok = github.credential_ref.starts_with("keychain:")
        && github.credential_ref.len() > "keychain:".len();
    if repo_ok && ref_ok && github.scale_set_name == "ubuntu-26.04-scale-set" {
        Ok(())
    } else {
        Err(HostError::Config)
    }
}

fn validate_docker(docker: &DockerConfig) -> Result<(), HostError> {
    if docker.platform != "linux/amd64" || docker.context.is_empty() {
        return Err(HostError::Config);
    }
    if docker.endpoint.starts_with("unix://") && !docker.endpoint.starts_with("unix:///") {
        return Err(HostError::Config);
    }
    if !docker.endpoint.starts_with("unix://") {
        return Err(HostError::Config);
    }
    Ok(())
}
