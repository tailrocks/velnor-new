//! Bounded local controller disconnect. Adopted Scale Sets and journal state remain.

use std::path::Path;
use std::process::ExitCode;

use super::drain::{requested_wait, write_flag_marker};

pub(super) fn disconnect(
    state: &Path,
    config_path: &Path,
    drain: bool,
    wait: bool,
    timeout_secs: Option<u64>,
    remove_local: bool,
) -> ExitCode {
    disconnect_for_os(
        state,
        config_path,
        drain,
        wait,
        timeout_secs,
        remove_local,
        std::env::consts::OS,
    )
}

pub(super) fn disconnect_for_os(
    state: &Path,
    config_path: &Path,
    drain: bool,
    wait: bool,
    timeout_secs: Option<u64>,
    remove_local: bool,
    os: &str,
) -> ExitCode {
    if !drain || !wait {
        eprintln!("disconnect requires --drain --wait");
        return ExitCode::from(2);
    }
    match os {
        "linux" => disconnect_linux(state, config_path, timeout_secs, remove_local),
        "macos" => disconnect_macos(state, timeout_secs, remove_local),
        _ => {
            eprintln!("disconnect is unavailable on this platform");
            ExitCode::from(1)
        }
    }
}

#[cfg(target_os = "linux")]
fn disconnect_linux(
    state: &Path,
    config_path: &Path,
    timeout_secs: Option<u64>,
    remove_local: bool,
) -> ExitCode {
    let local_binding = if remove_local {
        if let Ok(binding) = local_binding_for_removal(config_path) {
            Some(binding)
        } else {
            eprintln!("disconnect local removal requires a valid owned Linux binding");
            return ExitCode::from(1);
        }
    } else {
        None
    };

    linux_disconnect_with(
        timeout_secs,
        local_binding,
        |timeout| crate::service::linux::stop_for_disconnect(config_path, state, timeout),
        |binding| remove_local_binding(config_path, binding),
    )
}

#[cfg(not(target_os = "linux"))]
fn disconnect_linux(
    _state: &Path,
    _config_path: &Path,
    _timeout_secs: Option<u64>,
    _remove_local: bool,
) -> ExitCode {
    eprintln!("Linux disconnect is unavailable on this host");
    ExitCode::from(1)
}

fn disconnect_macos(state: &Path, timeout_secs: Option<u64>, remove_local: bool) -> ExitCode {
    if remove_local {
        eprintln!("--remove-local is available only on Linux");
        return ExitCode::from(1);
    }
    if !write_flag_marker(state, "drain") {
        eprintln!("failed to record the legacy drain marker");
        return ExitCode::from(1);
    }
    eprintln!(
        "legacy drain marker recorded; {} drain wait, physical quiescence, and remote Scale Set disconnection are not proven",
        requested_wait(timeout_secs)
    );
    ExitCode::from(1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalBindingRemoval {
    config_text: String,
    credential_ref: String,
}

fn local_binding_for_removal(config_path: &Path) -> Result<LocalBindingRemoval, ()> {
    use velnor_runner_host::{
        HostConfig, HostPlatform, read_host_config_file, validate_host_config_target,
    };

    validate_host_config_target(config_path, HostPlatform::Linux).map_err(|_| ())?;
    let config_text = read_host_config_file(config_path, HostPlatform::Linux)
        .map_err(|_| ())?
        .ok_or(())?;
    let config = HostConfig::parse(&config_text).map_err(|_| ())?;
    config
        .validate_for_host(HostPlatform::Linux)
        .map_err(|_| ())?;
    Ok(LocalBindingRemoval {
        credential_ref: config.github.credential_ref,
        config_text,
    })
}

fn remove_local_binding(config_path: &Path, binding: &LocalBindingRemoval) -> Result<(), ()> {
    use velnor_runner_host::{HostPlatform, remove_configured_secret, remove_host_config_file};

    remove_local_binding_with(
        || {
            remove_host_config_file(config_path, &binding.config_text, HostPlatform::Linux)
                .map_err(|_| ())
        },
        || remove_configured_secret(&binding.credential_ref).map_err(|_| ()),
    )
}

fn remove_local_binding_with(
    remove_config: impl FnOnce() -> Result<(), ()>,
    remove_credential: impl FnOnce() -> Result<(), ()>,
) -> Result<(), ()> {
    // Remove the exact unchanged config first. If it changed after the stop,
    // retain the credential it may now reference.
    remove_config()?;
    remove_credential()
}

fn linux_disconnect_with<B>(
    timeout_secs: Option<u64>,
    local_binding: Option<B>,
    stop_and_verify: impl FnOnce(Option<u64>) -> ExitCode,
    remove_local: impl FnOnce(&B) -> Result<(), ()>,
) -> ExitCode {
    let stop_status = stop_and_verify(timeout_secs);
    if stop_status != ExitCode::SUCCESS {
        return stop_status;
    }
    let local_removed = if let Some(binding) = local_binding {
        if remove_local(&binding).is_err() {
            eprintln!("disconnect stopped the controller but local removal is incomplete");
            return ExitCode::from(1);
        }
        true
    } else {
        false
    };
    if local_removed {
        println!(
            "controller_disconnected_locally; remote_scale_set_retained; journal_state_retained; local_config_and_credential_removed"
        );
    } else {
        println!(
            "controller_disconnected_locally; remote_scale_set_retained; journal_state_retained; local_config_and_credential_retained"
        );
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
