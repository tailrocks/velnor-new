use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use velnor_runner_host::{
    DaemonLock, HostConfig, HostError, LaunchReport, launch_blocking, load_secret,
};

const RETRY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DaemonIntent {
    Wait,

    Listen,

    Err,
}

#[must_use]
pub(crate) fn daemon_intent(toml_text: Option<&str>) -> DaemonIntent {
    match toml_text {
        None => DaemonIntent::Wait,
        Some(text) => match HostConfig::parse(text) {
            Ok(_) => DaemonIntent::Listen,
            Err(_) => DaemonIntent::Err,
        },
    }
}

pub(crate) fn run_daemon(state: &Path) -> ExitCode {
    let Ok(lock) = DaemonLock::try_acquire(&state.join("daemon.lock")) else {
        eprintln!("daemon already running");
        return ExitCode::from(1);
    };
    serve(state, &lock)
}

fn serve(state: &Path, lock: &DaemonLock) -> ExitCode {
    if !lock.is_held() {
        return ExitCode::from(1);
    }
    loop {
        if !lock.is_held() {
            return ExitCode::from(1);
        }
        step(state);
    }
}

fn step(state: &Path) {
    let text = std::fs::read_to_string(state.join("host.toml")).ok();
    match daemon_intent(text.as_deref()) {
        DaemonIntent::Wait => pause(),
        DaemonIntent::Err => {
            eprintln!("invalid config");
            pause();
        }
        DaemonIntent::Listen => listen_config(state, text.as_deref()),
    }
}

fn listen_config(state: &Path, text: Option<&str>) {
    let Some(text) = text else {
        pause();
        return;
    };
    let Ok(config) = HostConfig::parse(text) else {
        eprintln!("invalid config");
        pause();
        return;
    };
    drive(state, &config);
}

fn drive(state: &Path, config: &HostConfig) {
    let Some((owner, repo)) = split_repo(&config.github.repository) else {
        eprintln!("invalid config");
        pause();
        return;
    };
    let secret = match load_configured_secret(config, load_secret) {
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
        config.host.max_jobs,
        match config.resource_budget() {
            Ok(budget) => budget,
            Err(error) => {
                eprintln!("{error}");
                pause();
                return;
            }
        },
    ) {
        Ok(report) => finish_launch(&report),
        Err(error) => {
            eprintln!("{error}");
            pause();
        }
    }
}

fn load_configured_secret<T, F>(config: &HostConfig, load: F) -> Result<T, HostError>
where
    F: FnOnce(&str, &str) -> Result<T, HostError>,
{
    let reference = &config.github.credential_ref;
    load(reference.service(), reference.account())
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
