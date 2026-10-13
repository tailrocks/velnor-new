//! `daemon run` holds one lock and calls `launch_once`.

use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use velnor_runner_host::{DaemonLock, HostConfig, HostPlatform, load_configured_secret};
use velnor_runner_launch::{LaunchReport, launch_blocking};

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
    let Ok(lock) = DaemonLock::try_acquire(&state.join("daemon.lock")) else {
        eprintln!("daemon already running");
        return ExitCode::from(1);
    };
    serve(state, config_path, &lock)
}

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
enum ConfigReadError {
    Invalid,
    Unreadable,
}

fn read_daemon_config(
    config_path: &Path,
    platform: HostPlatform,
) -> Result<Option<HostConfig>, ConfigReadError> {
    let text = match std::fs::read_to_string(config_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ConfigReadError::Unreadable),
    };
    let config = HostConfig::parse(&text).map_err(|_| ConfigReadError::Invalid)?;
    config
        .validate_for_host(platform)
        .map_err(|_| ConfigReadError::Invalid)?;
    Ok(Some(config))
}

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

fn listen_config(state: &Path, config: &HostConfig) -> bool {
    if !daemon_backend_supported(native_host_platform()) {
        eprintln!("Linux Scale Set admission is not enabled in this build");
        return false;
    }
    drive(state, config);
    true
}

const fn daemon_backend_supported(platform: HostPlatform) -> bool {
    matches!(platform, HostPlatform::Macos)
}

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

fn split_repo(repository: &str) -> Option<(&str, &str)> {
    let (owner, repo) = repository.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner, repo))
}

fn pause() {
    thread::sleep(RETRY);
}

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
