//! `daemon run` holds one lock and selects the platform-owned coordinator.

use std::path::Path;
use std::process::ExitCode;
#[cfg(target_os = "macos")]
use std::thread;
#[cfg(target_os = "macos")]
use std::time::Duration;

use velnor_runner_host::DaemonLock;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
use velnor_runner_host::HostPlatform;
#[cfg(target_os = "macos")]
use velnor_runner_host::load_configured_secret;
#[cfg(target_os = "linux")]
use velnor_runner_host::read_validated_host_config_snapshot;
#[cfg(target_os = "linux")]
use velnor_runner_host::validate_protected_state_directory;
#[cfg(any(target_os = "macos", test))]
use velnor_runner_host::{HostConfig, read_host_config_file};
#[cfg(target_os = "macos")]
use velnor_runner_launch::{LaunchReport, launch_blocking};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod shutdown_signal;

#[cfg(target_os = "macos")]
const RETRY: Duration = Duration::from_secs(5);

/// Missing file waits. Valid TOML listens. Rejected TOML is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum DaemonIntent {
    /// `host.toml` is absent.
    Wait,
    /// Parsed config can drive one launch.
    Listen,
    /// The file is not a valid host config.
    Err,
}

/// Classify `host.toml` text. `None` means the file is missing.
#[must_use]
#[cfg(test)]
pub(crate) fn daemon_intent(toml_text: Option<&str>) -> DaemonIntent {
    match toml_text {
        None => DaemonIntent::Wait,
        Some(text) => match HostConfig::parse(text) {
            Ok(_) => DaemonIntent::Listen,
            Err(_) => DaemonIntent::Err,
        },
    }
}

/// Hold `daemon.lock` and launch until the lock is lost.
pub(crate) fn run_daemon(state: &Path, config_path: &Path) -> ExitCode {
    #[cfg(target_os = "linux")]
    {
        match run_linux_after_snapshot(
            || {
                read_validated_host_config_snapshot(config_path, HostPlatform::Linux)
                    .map_err(|_| ())
            },
            |snapshot| linux::prepare(state, snapshot).map_err(|_| ()),
            || validate_protected_state_directory(state).map_err(|_| ()),
            |state_directory| DaemonLock::try_acquire_in(state_directory).map_err(|_| ()),
            |prepared, state_directory, lock| linux::run(&lock, prepared, state_directory),
        ) {
            Ok(code) => code,
            Err(LinuxStartupFailure::Configuration) => {
                eprintln!("Linux host configuration unavailable or invalid");
                ExitCode::from(1)
            }
            Err(LinuxStartupFailure::StateDirectory) => {
                eprintln!("Linux state directory unavailable or unsafe");
                ExitCode::from(1)
            }
            Err(LinuxStartupFailure::Context) => {
                eprintln!("Linux daemon context invalid");
                ExitCode::from(1)
            }
            Err(LinuxStartupFailure::Lock) => {
                eprintln!("daemon already running");
                ExitCode::from(1)
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let Ok(lock) = DaemonLock::try_acquire(&state.join("daemon.lock")) else {
            eprintln!("daemon already running");
            return ExitCode::from(1);
        };
        serve(state, config_path, &lock)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (state, config_path);
        eprintln!("daemon is unsupported on this platform");
        ExitCode::from(1)
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxStartupFailure {
    Configuration,
    Context,
    StateDirectory,
    Lock,
}

/// Read trusted config and derive the coordinator context before validating
/// the state directory, then acquire the lock through its retained descriptor.
#[cfg(target_os = "linux")]
fn run_linux_after_snapshot<T, C, S, L>(
    read_snapshot: impl FnOnce() -> Result<Option<T>, ()>,
    prepare_context: impl FnOnce(T) -> Result<C, ()>,
    validate_state: impl FnOnce() -> Result<S, ()>,
    acquire_lock: impl FnOnce(&S) -> Result<L, ()>,
    run: impl FnOnce(C, S, L) -> ExitCode,
) -> Result<ExitCode, LinuxStartupFailure> {
    let snapshot = read_snapshot()
        .map_err(|()| LinuxStartupFailure::Configuration)?
        .ok_or(LinuxStartupFailure::Configuration)?;
    let context = prepare_context(snapshot).map_err(|()| LinuxStartupFailure::Context)?;
    let state_directory = validate_state().map_err(|()| LinuxStartupFailure::StateDirectory)?;
    let lock = acquire_lock(&state_directory).map_err(|()| LinuxStartupFailure::Lock)?;
    Ok(run(context, state_directory, lock))
}

#[cfg(target_os = "macos")]
fn serve(state: &Path, config_path: &Path, lock: &DaemonLock) -> ExitCode {
    if !lock.is_held() {
        return ExitCode::from(1);
    }
    loop {
        if !lock.is_held() {
            return ExitCode::from(1);
        }
        if !step(state, config_path) {
            return ExitCode::from(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(any(target_os = "macos", test))]
enum ConfigReadError {
    Invalid,
    Unreadable,
}

#[cfg(any(target_os = "macos", test))]
fn read_daemon_config(
    config_path: &Path,
    platform: HostPlatform,
) -> Result<Option<HostConfig>, ConfigReadError> {
    read_daemon_config_with(config_path, platform, |path, host_platform| {
        read_host_config_file(path, host_platform).map_err(|_| ())
    })
}

#[cfg(any(target_os = "macos", test))]
fn read_daemon_config_with(
    config_path: &Path,
    platform: HostPlatform,
    read: impl FnOnce(&Path, HostPlatform) -> Result<Option<String>, ()>,
) -> Result<Option<HostConfig>, ConfigReadError> {
    let text = read(config_path, platform).map_err(|()| ConfigReadError::Unreadable)?;
    let Some(text) = text else {
        return Ok(None);
    };
    parse_daemon_config(&text, platform).map(Some)
}

#[cfg(any(target_os = "macos", test))]
fn parse_daemon_config(text: &str, platform: HostPlatform) -> Result<HostConfig, ConfigReadError> {
    let config = HostConfig::parse(text).map_err(|_| ConfigReadError::Invalid)?;
    config
        .validate_for_host(platform)
        .map_err(|_| ConfigReadError::Invalid)?;
    Ok(config)
}

#[cfg(target_os = "macos")]
fn step(state: &Path, config_path: &Path) -> bool {
    match read_daemon_config(config_path, native_host_platform()) {
        Ok(None) => pause(),
        Err(ConfigReadError::Invalid | ConfigReadError::Unreadable) => {
            eprintln!("invalid config");
            pause();
        }
        Ok(Some(config)) => return listen_config(state, &config),
    }
    true
}

#[cfg(target_os = "macos")]
fn listen_config(state: &Path, config: &HostConfig) -> bool {
    if !daemon_backend_supported(native_host_platform()) {
        eprintln!("Linux Scale Set admission is not enabled in this build");
        return false;
    }
    drive(state, config);
    true
}

#[cfg(any(target_os = "macos", test))]
const fn daemon_backend_supported(platform: HostPlatform) -> bool {
    matches!(platform, HostPlatform::Macos)
}

#[cfg(target_os = "macos")]
fn drive(state: &Path, config: &HostConfig) {
    let Some((owner, repo)) = split_repo(&config.github.repository) else {
        eprintln!("invalid config");
        pause();
        return;
    };
    let secret = match load_configured_secret(&config.github.credential_ref) {
        Ok(secret) => secret,
        Err(error) => {
            eprintln!("{error}");
            pause();
            return;
        }
    };
    let Some(pat) = credential_text(secret.as_slice()) else {
        eprintln!("keychain");
        pause();
        return;
    };
    match launch_blocking(
        pat,
        owner,
        repo,
        &config.docker.endpoint,
        &state.join("launch.db"),
        config.max_jobs(),
    ) {
        Ok(report) => finish_launch(&report),
        Err(error) => {
            eprintln!("{error}");
            pause();
        }
    }
}

#[cfg(target_os = "macos")]
fn finish_launch(report: &LaunchReport) {
    println!("set_id={} workers={}", report.set_id, report.workers.len());
    if report.workers.is_empty() {
        pause();
    }
}

fn credential_text(secret: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(secret).ok()?;
    let pat = text.trim();
    if pat.is_empty() { None } else { Some(pat) }
}

#[cfg(target_os = "macos")]
fn split_repo(repository: &str) -> Option<(&str, &str)> {
    let (owner, repo) = repository.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner, repo))
}

#[cfg(target_os = "macos")]
fn pause() {
    thread::sleep(RETRY);
}

#[cfg(target_os = "macos")]
const fn native_host_platform() -> HostPlatform {
    #[cfg(target_os = "linux")]
    {
        HostPlatform::Linux
    }
    #[cfg(target_os = "macos")]
    {
        HostPlatform::Macos
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        HostPlatform::Linux
    }
}

#[cfg(test)]
mod tests;
