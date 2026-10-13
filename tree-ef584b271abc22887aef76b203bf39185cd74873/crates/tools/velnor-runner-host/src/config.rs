//! Secret-free host TOML. Credentials are references to host-only stores.

use serde::Deserialize;

use self::validation::{
    keychain_ref, split_repository, validate_docker, validate_github, validate_trust,
};
use crate::HostError;

mod validation;

/// Top-level controller file. Schema 1 accepts legacy macOS fields and
/// requires the explicit trust, scope, group, and image profile for Linux.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    /// Must be 1.
    pub schema: u32,
    /// Optional provenance hint written by `connect`. This is not a security
    /// proof; `disconnect` also requires the platform's exact path, file
    /// owner and mode, unchanged contents, completed drain, and stopped unit.
    #[serde(default)]
    pub managed_by: Option<String>,
    /// Repository, Scale Set, group, and host-only credential reference.
    pub github: GithubSection,
    /// Capacity and host platform.
    pub host: HostLimits,
    /// Selected Docker context and container platform.
    pub docker: DockerConfig,
    /// Required private-repository/event policy on supported controller hosts.
    #[serde(default)]
    pub trust: Option<JobTrustPolicy>,
    /// Required image profile for the Linux controller backend.
    #[serde(default)]
    pub runner: Option<RunnerConfig>,
}

/// Host OS backend. This is separate from the runner container platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostPlatform {
    /// Existing per-user `LaunchAgent` controller.
    Macos,
    /// systemd-managed Linux controller.
    Linux,
}

/// Registration scope supported by this product configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationScopeKind {
    /// Repository-scoped GitHub App/PAT registration.
    Repository,
}

/// Exact Scale Set identity for registration and scheduling calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaleSetBinding {
    /// Registration scope.
    pub scope: RegistrationScopeKind,
    /// Repository owner.
    pub owner: String,
    /// Repository name.
    pub repository: String,
    /// Scale Set name.
    pub scale_set_name: String,
    /// Runner group id.
    pub runner_group_id: i64,
    /// Runner group display name.
    pub runner_group_name: String,
    /// Immutable runner profile associated with this selector. Legacy macOS
    /// configurations retain `None`; Linux requires an explicit profile.
    pub runner_image_profile: Option<String>,
}

/// Explicit workflow admission policy. It never substitutes label selection
/// for repository, event, or pull-request-source validation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobTrustPolicy {
    /// Exact `owner/repository` names accepted by this controller.
    pub allowed_repositories: Vec<String>,
    /// Exact GitHub event names accepted by this controller.
    pub allowed_events: Vec<String>,
    /// Exact workflow paths from the GitHub Actions run API.
    pub allowed_workflow_paths: Vec<String>,
    /// Must remain false for the privileged per-job `DinD` host.
    pub allow_forks: bool,
}

impl JobTrustPolicy {
    /// Admit a message only when its exact repository and event are listed.
    /// Forks are accepted only when their source repository exactly matches
    /// the allowed base repository; a missing source fails closed.
    #[must_use]
    pub fn allows(
        &self,
        owner: &str,
        repository: &str,
        event: &str,
        head_repository: Option<&str>,
        workflow_path: &str,
    ) -> bool {
        let full_name = format!("{owner}/{repository}");
        self.allowed_repositories
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&full_name))
            && self.allowed_events.iter().any(|allowed| allowed == event)
            && self
                .allowed_workflow_paths
                .iter()
                .any(|allowed| allowed == workflow_path)
            && !self.allow_forks
            && head_repository.is_some_and(|head| head.eq_ignore_ascii_case(&full_name))
    }
}

/// GitHub Scale Set binding. The credential is a reference, never a token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubSection {
    /// `owner/name`.
    pub repository: String,
    /// Scale Set name.
    pub scale_set_name: String,
    /// Host-only credential reference (`keychain:` on macOS or
    /// `systemd-credential:github-token` on Linux).
    pub credential_ref: String,
    /// Explicit registration scope on newly configured controllers.
    #[serde(default)]
    pub registration_scope: Option<RegistrationScopeKind>,
    /// Explicit runner group id on newly configured controllers.
    #[serde(default)]
    pub runner_group_id: Option<i64>,
    /// Explicit runner group name on newly configured controllers.
    #[serde(default)]
    pub runner_group_name: Option<String>,
}

/// Controller limits and host-specific settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostLimits {
    /// Total permits. Linux requires an explicitly measured value; macOS
    /// retains its historical default of one.
    #[serde(default)]
    pub max_jobs: Option<u32>,
    /// Explicit controller OS for newly configured deployments. Missing means
    /// the legacy macOS configuration format, never Linux.
    #[serde(default)]
    pub platform: Option<HostPlatform>,
    /// Finite upper bound used by `drain --wait` and service stop.
    #[serde(default)]
    pub drain_timeout_secs: Option<u64>,
}

/// Immutable runner/DinD allowlist key. Digest resolution is owned by
/// `docker_spec`; configuration cannot supply arbitrary image references.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerConfig {
    /// Supported host image profile, such as `ubuntu-24.04-amd64`.
    pub image_profile: String,
}

/// Docker binding.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockerConfig {
    /// Context name. Not a provider switch.
    pub context: String,
    /// Runner platform. `linux/amd64` only.
    pub platform: String,
    /// `unix://` socket. `tcp://` and `ssh://` are rejected.
    pub endpoint: String,
}

impl HostConfig {
    /// Parse and structurally validate secret-free TOML.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] for malformed schema, secret values,
    /// invalid identities, or unsafe Docker endpoints.
    pub fn parse(text: &str) -> Result<Self, HostError> {
        let config: Self = toml::from_str(text).map_err(|_| HostError::Config)?;
        config.validate()?;
        Ok(config)
    }

    /// Enforce the selected host backend and its required explicit policy.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when the config is for another platform
    /// or omits platform-specific trust, credential, group, or profile data.
    pub fn validate_for_host(&self, platform: HostPlatform) -> Result<(), HostError> {
        let configured = self.host.platform;
        let legacy_macos = platform == HostPlatform::Macos && configured.is_none();
        if !legacy_macos && configured != Some(platform) {
            return Err(HostError::Config);
        }
        let has_group =
            self.github.runner_group_id.is_some() && self.github.runner_group_name.is_some();
        if !legacy_macos
            && (!has_group
                || self.github.registration_scope != Some(RegistrationScopeKind::Repository))
        {
            return Err(HostError::Config);
        }
        if let Some(trust) = &self.trust {
            validate_trust(trust, &self.github.repository)?;
        }
        if platform == HostPlatform::Linux {
            let trust = self.trust.as_ref().ok_or(HostError::Config)?;
            if self.github.credential_ref != "systemd-credential:github-token"
                || trust.allow_forks
                || self.host.drain_timeout_secs.is_none()
                || self.host.max_jobs.is_none()
            {
                return Err(HostError::Config);
            }
            let runner = self.runner.as_ref().ok_or(HostError::Config)?;
            crate::docker_spec::resolve_runner_profile(
                &runner.image_profile,
                &self.github.scale_set_name,
            )
            .map_err(|_| HostError::Config)?;
        } else if !keychain_ref(&self.github.credential_ref) || self.runner.is_some() {
            return Err(HostError::Config);
        }
        Ok(())
    }

    /// Build the owned binding shared by lookup, session, and launch paths.
    /// Legacy macOS schema uses its historical repository/default-group values.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when owner/repository or group identity is
    /// invalid or incomplete.
    pub fn scale_set_binding(&self) -> Result<ScaleSetBinding, HostError> {
        let (owner, repository) = split_repository(&self.github.repository)?;
        let (runner_group_id, runner_group_name) = match (
            self.github.runner_group_id,
            self.github.runner_group_name.as_deref(),
        ) {
            (Some(id), Some(name)) => (id, name),
            (None, None) if self.host.platform.is_none() => (1, "Default"),
            _ => return Err(HostError::Config),
        };
        let binding = ScaleSetBinding {
            scope: self
                .github
                .registration_scope
                .unwrap_or(RegistrationScopeKind::Repository),
            owner: owner.to_owned(),
            repository: repository.to_owned(),
            scale_set_name: self.github.scale_set_name.clone(),
            runner_group_id,
            runner_group_name: runner_group_name.to_owned(),
            runner_image_profile: self
                .runner
                .as_ref()
                .map(|runner| runner.image_profile.clone()),
        };
        if binding.runner_group_id <= 0 || binding.runner_group_name.trim().is_empty() {
            return Err(HostError::Config);
        }
        Ok(binding)
    }

    /// Return an owned explicit job-trust policy.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when the policy is absent or malformed.
    pub fn job_trust_policy(&self) -> Result<JobTrustPolicy, HostError> {
        let policy = self.trust.clone().ok_or(HostError::Config)?;
        validate_trust(&policy, &self.github.repository)?;
        Ok(policy)
    }

    /// Return the configured finite drain timeout.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when a required timeout is absent or zero.
    pub fn drain_timeout_secs(&self) -> Result<u64, HostError> {
        let seconds = self.host.drain_timeout_secs.ok_or(HostError::Config)?;
        if seconds == 0 {
            return Err(HostError::Config);
        }
        Ok(seconds)
    }

    /// Whether this config carries the marker written by the supported
    /// `connect` command and can be considered for local disconnect cleanup.
    #[must_use]
    pub fn is_connect_managed(&self) -> bool {
        self.managed_by.as_deref() == Some("velnor-host-connect-v1")
    }

    /// Return the host-wide worker limit. Legacy macOS configuration defaults
    /// to one; Linux configurations are rejected unless the value is explicit.
    #[must_use]
    pub const fn max_jobs(&self) -> u32 {
        match self.host.max_jobs {
            Some(value) => value,
            None => 1,
        }
    }

    fn validate(&self) -> Result<(), HostError> {
        if self.schema != 1 || self.host.max_jobs == Some(0) {
            return Err(HostError::Config);
        }
        if self
            .managed_by
            .as_deref()
            .is_some_and(|value| value != "velnor-host-connect-v1")
        {
            return Err(HostError::Config);
        }
        if self.host.drain_timeout_secs == Some(0) {
            return Err(HostError::Config);
        }
        validate_github(&self.github)?;
        validate_docker(&self.docker)?;
        if let Some(trust) = &self.trust {
            validate_trust(trust, &self.github.repository)?;
        }
        if let Some(runner) = &self.runner
            && (runner.image_profile.trim().is_empty()
                || runner.image_profile.chars().any(char::is_whitespace))
        {
            return Err(HostError::Config);
        }
        if self.github.runner_group_id.is_some() != self.github.runner_group_name.is_some() {
            return Err(HostError::Config);
        }
        if self.github.runner_group_id.is_some_and(|id| id <= 0)
            || self
                .github
                .runner_group_name
                .as_deref()
                .is_some_and(|name| name.trim().is_empty() || name.chars().any(char::is_control))
        {
            return Err(HostError::Config);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
