//! Validate a binding before storing its host-only credential.

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_runner_host::{
    ConnectPlan, EnsureError, HostConfig, HostError, HostPlatform, JobTrustPolicy,
    MAX_LINUX_DRAIN_TIMEOUT_SECS, connect_plan, discover_product_scale_set,
    persist_host_config_file, read_host_config_file, read_secret, remove_host_config_file,
    store_configured_secret, validate_host_config_target,
};

use crate::service::ControllerServiceState;

mod binding;
mod format;
mod platform;
mod trust_policy;

use self::binding::{BindingAndTrust, validate_binding_and_trust};
use self::format::{credential_reference, toml_string};
use self::platform::host_platform;
#[cfg(test)]
pub(super) use self::platform::host_platform_for;
use self::trust_policy::{
    append_legacy_platform_trust, append_trust_policy, load_linux_trust_policy,
};

pub(super) struct ConnectRequest<'a> {
    pub(super) config_path: &'a Path,
    pub(super) repo: &'a str,
    pub(super) scale_set: &'a str,
    pub(super) platform: &'a str,
    pub(super) host_platform: Option<&'a str>,
    pub(super) registration_scope: Option<&'a str>,
    pub(super) runner_group_id: Option<i64>,
    pub(super) runner_group_name: Option<&'a str>,
    pub(super) allowed_events: &'a [String],
    pub(super) allowed_workflow_paths: &'a [String],
    pub(super) trust_policy_file: Option<PathBuf>,
    pub(super) image_profile: Option<&'a str>,
    pub(super) max_jobs: Option<u32>,
    pub(super) drain_timeout_secs: Option<u64>,
    pub(super) docker_context: Option<&'a str>,
    pub(super) endpoint: Option<&'a str>,
}

struct ConnectSettings<'a> {
    host_platform: &'static str,
    group: Option<(i64, &'a str)>,
    image_profile: Option<&'a str>,
    max_jobs: u32,
    legacy_macos: bool,
    trust_policy: Option<JobTrustPolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectError {
    Config,
    Rejected,
    GroupPolicyUnavailable,
    TrustPolicy,
    ServiceInUse,
    ServiceStateUnknown,
    Secret(HostError),
    Write,
    Rollback,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config => formatter.write_str("invalid host configuration"),
            Self::Rejected => formatter.write_str("connection rejected"),
            Self::GroupPolicyUnavailable => formatter.write_str(
                "runner-group workflow policy could not be verified; no new configuration or credential was stored",
            ),
            Self::TrustPolicy => formatter.write_str(
                "Linux requires a valid exact trust-policy JSON file matching the selected repository, events, and workflow paths",
            ),
            Self::ServiceInUse => {
                formatter.write_str("controller service is running or transitioning")
            }
            Self::ServiceStateUnknown => {
                formatter.write_str("cannot prove controller service is stopped")
            }
            Self::Secret(error) => std::fmt::Display::fmt(error, formatter),
            Self::Write => formatter.write_str("host credential or configuration write failed"),
            Self::Rollback => formatter.write_str(
                "credential storage failed and the new configuration could not be rolled back",
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ConnectFileOps {
    pub(super) read: fn(&Path) -> Result<Option<String>, ConnectError>,
    pub(super) persist: fn(&Path, &str) -> Result<(), ConnectError>,
    pub(super) remove: fn(&Path, &str) -> Result<(), ConnectError>,
}

pub(super) fn connect(request: &ConnectRequest<'_>) -> ExitCode {
    let Some(platform) = host_platform() else {
        eprintln!("connect is unavailable on this host");
        return ExitCode::from(1);
    };
    if validate_host_config_target(request.config_path, platform).is_err() {
        eprintln!("configuration path, service identity, or directory is not ready");
        return ExitCode::from(1);
    }
    let mut stdin = std::io::stdin();
    match connect_with_service_state(
        &mut stdin,
        request,
        crate::service::controller_service_state(),
        installed_file_ops(),
        |token, config| {
            let binding = config
                .scale_set_binding()
                .map_err(|_| ConnectError::Config)?;
            discover_product_scale_set(token, &binding)
                .map(|_| ())
                .map_err(map_discovery_error)
        },
        |config, secret| {
            store_configured_secret(&config.github.credential_ref, secret)
                .map_err(ConnectError::Secret)
        },
    ) {
        Ok(()) => {
            println!("configuration_saved path={}", request.config_path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

pub(super) fn map_discovery_error(error: EnsureError) -> ConnectError {
    match error {
        EnsureError::GroupPolicyUnavailable => ConnectError::GroupPolicyUnavailable,
        _ => ConnectError::Rejected,
    }
}

pub(super) fn connect_with<R, V, S>(
    input: &mut R,
    request: &ConnectRequest<'_>,
    file_ops: ConnectFileOps,
    validate_remote: V,
    store_secret: S,
) -> Result<(), ConnectError>
where
    R: Read,
    V: FnOnce(&str, &HostConfig) -> Result<(), ConnectError>,
    S: FnOnce(&HostConfig, &[u8]) -> Result<(), ConnectError>,
{
    let actual_host = host_platform().ok_or(ConnectError::Config)?;
    let config_text = sample_config_for(request, actual_host)?;
    let config = HostConfig::parse(&config_text).map_err(|_| ConnectError::Config)?;
    config
        .validate_for_host(actual_host)
        .map_err(|_| ConnectError::Config)?;
    let configuration_exists = check_existing_binding(request.config_path, &config, file_ops)?;
    let secret = read_secret(input).map_err(ConnectError::Secret)?;
    let token = std::str::from_utf8(secret.as_slice())
        .map_err(|_| ConnectError::Secret(HostError::Keychain))?;
    let token = token.trim();
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return Err(ConnectError::Secret(HostError::Keychain));
    }
    validate_remote(token, &config)?;
    if !configuration_exists {
        (file_ops.persist)(request.config_path, &config_text).map_err(|_| ConnectError::Write)?;
    }
    if let Err(error) = store_secret(&config, secret.as_slice()) {
        if !configuration_exists && (file_ops.remove)(request.config_path, &config_text).is_err() {
            return Err(ConnectError::Rollback);
        }
        return Err(error);
    }
    Ok(())
}

pub(super) fn connect_with_service_state<R, V, S>(
    input: &mut R,
    request: &ConnectRequest<'_>,
    service_state: ControllerServiceState,
    file_ops: ConnectFileOps,
    validate_remote: V,
    store_secret: S,
) -> Result<(), ConnectError>
where
    R: Read,
    V: FnOnce(&str, &HostConfig) -> Result<(), ConnectError>,
    S: FnOnce(&HostConfig, &[u8]) -> Result<(), ConnectError>,
{
    match service_state {
        ControllerServiceState::Stopped => {
            connect_with(input, request, file_ops, validate_remote, store_secret)
        }
        ControllerServiceState::InUse => Err(ConnectError::ServiceInUse),
        ControllerServiceState::Unknown => Err(ConnectError::ServiceStateUnknown),
    }
}

fn check_existing_binding(
    path: &Path,
    request: &HostConfig,
    file_ops: ConnectFileOps,
) -> Result<bool, ConnectError> {
    let Some(raw) = (file_ops.read)(path)? else {
        return Ok(false);
    };
    let current = HostConfig::parse(&raw).map_err(|_| ConnectError::Config)?;
    if connect_plan(Some(&current), request) == ConnectPlan::Rejected {
        return Err(ConnectError::Rejected);
    }
    Ok(true)
}

fn installed_file_ops() -> ConnectFileOps {
    ConnectFileOps {
        read: read_installed_config,
        persist: persist_installed_config,
        remove: remove_installed_config,
    }
}

fn read_installed_config(path: &Path) -> Result<Option<String>, ConnectError> {
    read_host_config_file(path, host_platform().ok_or(ConnectError::Config)?)
        .map_err(|_| ConnectError::Write)
}

fn persist_installed_config(path: &Path, text: &str) -> Result<(), ConnectError> {
    persist_host_config_file(path, text, host_platform().ok_or(ConnectError::Config)?)
        .map_err(|_| ConnectError::Write)
}

fn remove_installed_config(path: &Path, expected: &str) -> Result<(), ConnectError> {
    remove_host_config_file(path, expected, host_platform().ok_or(ConnectError::Config)?)
        .map_err(|_| ConnectError::Write)
}

pub(super) fn sample_config_for(
    request: &ConnectRequest<'_>,
    actual_host: HostPlatform,
) -> Result<String, ConnectError> {
    let settings = connect_settings(request, actual_host)?;
    render_config(request, &settings)
}

fn connect_settings<'a>(
    request: &ConnectRequest<'a>,
    actual_host: HostPlatform,
) -> Result<ConnectSettings<'a>, ConnectError> {
    if request.drain_timeout_secs == Some(0) || request.max_jobs == Some(0) {
        return Err(ConnectError::Config);
    }
    let default_platform = match actual_host {
        HostPlatform::Linux => "linux",
        HostPlatform::Macos => "macos",
    };
    let host_platform = match request.host_platform.unwrap_or(default_platform) {
        "linux" if actual_host == HostPlatform::Linux => "linux",
        "macos" if actual_host == HostPlatform::Macos => "macos",
        _ => return Err(ConnectError::Config),
    };
    let linux = host_platform == "linux";
    if linux
        && request
            .drain_timeout_secs
            .is_some_and(|timeout| timeout > MAX_LINUX_DRAIN_TIMEOUT_SECS)
    {
        return Err(ConnectError::Config);
    }
    let BindingAndTrust {
        group,
        explicit_binding,
    } = validate_binding_and_trust(request, linux)?;
    let image = match (linux, request.image_profile) {
        (true, Some(profile)) => Some(profile),
        (true, None) | (false, Some(_)) => return Err(ConnectError::Config),
        (false, None) => None,
    };
    let legacy_macos = !linux
        && !explicit_binding
        && request.allowed_events.is_empty()
        && request.allowed_workflow_paths.is_empty();
    let trust_policy = if linux {
        Some(load_linux_trust_policy(request)?)
    } else if request.trust_policy_file.is_some() {
        return Err(ConnectError::TrustPolicy);
    } else {
        None
    };
    Ok(ConnectSettings {
        host_platform,
        group,
        image_profile: image,
        max_jobs: request.max_jobs.unwrap_or(1),
        legacy_macos,
        trust_policy,
    })
}

fn render_config(
    request: &ConnectRequest<'_>,
    settings: &ConnectSettings<'_>,
) -> Result<String, ConnectError> {
    let repository = toml_string(request.repo)?;
    let scale_set = toml_string(request.scale_set)?;
    let context = toml_string(request.docker_context.unwrap_or(
        if settings.host_platform == "macos" {
            "orbstack"
        } else {
            "system"
        },
    ))?;
    let endpoint = toml_string(request.endpoint.unwrap_or("unix:///var/run/docker.sock"))?;
    let platform = toml_string(request.platform)?;
    let credential_ref = credential_reference(settings.host_platform);
    let mut text = format!(
        "schema = 1\nmanaged_by = \"velnor-host-connect-v1\"\n[github]\nrepository = {repository}\nscale_set_name = {scale_set}\ncredential_ref = {credential_ref:?}\n"
    );
    append_group(&mut text, settings.group)?;
    text.push_str("[host]\n");
    if !settings.legacy_macos {
        writeln!(text, "platform = \"{}\"", settings.host_platform)
            .map_err(|_| ConnectError::Write)?;
    }
    writeln!(text, "max_jobs = {}", settings.max_jobs).map_err(|_| ConnectError::Write)?;
    if let Some(timeout) = request.drain_timeout_secs {
        writeln!(text, "drain_timeout_secs = {timeout}").map_err(|_| ConnectError::Write)?;
    }
    if let Some(policy) = settings.trust_policy.as_ref() {
        append_trust_policy(&mut text, Some(policy))?;
    } else {
        append_legacy_platform_trust(&mut text, request)?;
    }
    write!(
        text,
        "[docker]\ncontext = {context}\nplatform = {platform}\nendpoint = {endpoint}\n"
    )
    .map_err(|_| ConnectError::Write)?;
    if let Some(image) = settings.image_profile {
        write!(text, "[runner]\nimage_profile = {}\n", toml_string(image)?)
            .map_err(|_| ConnectError::Write)?;
    }
    Ok(text)
}

fn append_group(text: &mut String, group: Option<(i64, &str)>) -> Result<(), ConnectError> {
    let Some((id, name)) = group else {
        return Ok(());
    };
    let name = toml_string(name)?;
    write!(
        text,
        "registration_scope = \"repository\"\nrunner_group_id = {id}\nrunner_group_name = {name}\n"
    )
    .map_err(|_| ConnectError::Write)?;
    Ok(())
}
