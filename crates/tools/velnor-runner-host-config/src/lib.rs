//! Validated host configuration: secret-free TOML schema plus atomic file ownership.
//!
//! The schema and its platform validation plus the Linux service file
//! handling. Credential storage lives in the host keychain module, which
//! builds on the directory and ownership helpers re-exported here.

mod config;
mod config_file;

pub use config::{
    DockerConfig, GithubSection, HostConfig, HostLimits, HostPlatform, JobTrustPolicy,
    JobTrustRule, RegistrationScope, RegistrationScopeKind, ReusableWorkflowRule, RunnerConfig,
    ScaleSetBinding,
};
pub use config_file::{
    LINUX_CONFIG_PATH, MAX_HOST_CONFIG_BYTES, assign_owner, linux_service_group_id,
    open_systemd_credential_file, persist_host_config_file, read_host_config_bytes,
    read_host_config_file, remove_host_config_file, validate_host_config_target,
    validate_linux_directory,
};
pub use velnor_runner_journal::HostError;
