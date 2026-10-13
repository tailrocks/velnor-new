use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use velnor_runner_host::{
    HostConfig, HostPlatform, MAX_LINUX_DRAIN_TIMEOUT_SECS, read_host_config_file,
    validate_protected_state_directory,
};
use velnor_runner_launch::launch::control::DrainUnknown;

pub(super) fn write_flag_marker(state: &Path, name: &str) -> bool {
    std::fs::create_dir_all(state).is_ok() && std::fs::write(state.join(name), b"1").is_ok()
}

fn remove_flag(state: &Path, name: &str) -> ExitCode {
    let path = state.join(name);
    match std::fs::remove_file(path) {
        Ok(()) => eprintln!("legacy {name} marker removed; macOS admission state is not proven"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("legacy {name} marker absent; macOS admission state is not proven");
        }
        Err(_) => {
            eprintln!("failed to remove legacy {name} marker; macOS admission state is not proven");
        }
    }
    ExitCode::from(1)
}

pub(super) fn drain(
    state: &Path,
    config_path: &Path,
    wait: bool,
    timeout_secs: Option<u64>,
) -> ExitCode {
    drain_for_os(state, config_path, wait, timeout_secs, std::env::consts::OS)
}

pub(super) fn drain_for_os(
    state: &Path,
    config_path: &Path,
    wait: bool,
    timeout_secs: Option<u64>,
    os: &str,
) -> ExitCode {
    match os {
        "linux" => drain_linux(state, config_path, wait, timeout_secs),
        "macos" => {
            if !write_flag_marker(state, "drain") {
                eprintln!("failed to record the legacy drain marker");
                return ExitCode::from(1);
            }
            if wait {
                eprintln!(
                    "legacy drain marker recorded; active-daemon enforcement, drain wait, and quiescence are not proven"
                );
            } else {
                eprintln!(
                    "legacy drain marker recorded; active-daemon enforcement and quiescence are not proven"
                );
            }
            ExitCode::from(1)
        }
        _ => {
            eprintln!("drain is unavailable on this platform");
            ExitCode::from(1)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LinuxDrainSettings {
    pub(super) docker_endpoint: String,
    pub(super) configured_timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LinuxDrainStatus {
    Requested,
    RequestDeadline,
    RequestUnknown,
    RequestUnavailable,
    Drained,
    WaitDeadline,
    WaitUnknown(velnor_runner_launch::launch::control::DrainUnknown),
    InvalidDeadline,
}

fn linux_drain_settings(config_path: &Path) -> Result<LinuxDrainSettings, ()> {
    let text = read_host_config_file(config_path, HostPlatform::Linux)
        .map_err(|_| ())?
        .ok_or(())?;
    let config = HostConfig::parse(&text).map_err(|_| ())?;
    config
        .validate_for_host(HostPlatform::Linux)
        .map_err(|_| ())?;
    let configured_timeout_secs = config.drain_timeout_secs().map_err(|_| ())?;
    Ok(LinuxDrainSettings {
        docker_endpoint: config.docker.endpoint,
        configured_timeout_secs,
    })
}

fn drain_linux(
    state: &Path,
    config_path: &Path,
    wait: bool,
    timeout_secs: Option<u64>,
) -> ExitCode {
    let Ok(settings) = linux_drain_settings(config_path) else {
        eprintln!("Linux drain requires a valid protected host configuration");
        return ExitCode::from(1);
    };
    let Ok(protected_state) = validate_protected_state_directory(state) else {
        eprintln!("Linux drain requires a protected service state directory");
        return ExitCode::from(1);
    };
    let status = linux_drain_with(
        &protected_state,
        &state.join("launch.db"),
        wait,
        timeout_secs,
        &settings,
        velnor_runner_launch::launch::drain_observer::request_drain_protected_blocking,
        velnor_runner_launch::launch::drain_observer::wait_drained_protected_blocking,
    );
    report_linux_drain(status)
}

pub(super) fn linux_drain_with<S, R, W>(
    protected_state: &S,
    journal_path: &Path,
    wait: bool,
    timeout_secs: Option<u64>,
    settings: &LinuxDrainSettings,
    request: R,
    wait_drained: W,
) -> LinuxDrainStatus
where
    R: FnOnce(
        &S,
        &Path,
        Instant,
    ) -> Result<
        velnor_runner_launch::launch::control::DrainRequestOutcome,
        velnor_runner_launch::launch::control::ControlOpenError,
    >,
    W: FnOnce(&S, &Path, &str, Instant) -> velnor_runner_launch::launch::control::DrainOutcome,
{
    let seconds = timeout_secs.unwrap_or(settings.configured_timeout_secs);
    let valid_timeout = seconds > 0
        && seconds <= settings.configured_timeout_secs
        && seconds <= MAX_LINUX_DRAIN_TIMEOUT_SECS;
    let Some(deadline) = valid_timeout
        .then(|| Instant::now().checked_add(Duration::from_secs(seconds)))
        .flatten()
    else {
        return LinuxDrainStatus::InvalidDeadline;
    };
    let request_result = request(protected_state, journal_path, deadline);
    match request_result {
        Err(_) => LinuxDrainStatus::RequestUnavailable,
        Ok(velnor_runner_launch::launch::control::DrainRequestOutcome::DeadlineBeforeMutation) => {
            LinuxDrainStatus::RequestDeadline
        }
        Ok(velnor_runner_launch::launch::control::DrainRequestOutcome::UnknownAfterMutation) => {
            LinuxDrainStatus::RequestUnknown
        }
        Ok(velnor_runner_launch::launch::control::DrainRequestOutcome::Requested) if !wait => {
            LinuxDrainStatus::Requested
        }
        Ok(velnor_runner_launch::launch::control::DrainRequestOutcome::Requested) => {
            match wait_drained(
                protected_state,
                journal_path,
                &settings.docker_endpoint,
                deadline,
            ) {
                velnor_runner_launch::launch::control::DrainOutcome::Drained => {
                    LinuxDrainStatus::Drained
                }
                velnor_runner_launch::launch::control::DrainOutcome::Deadline => {
                    LinuxDrainStatus::WaitDeadline
                }
                velnor_runner_launch::launch::control::DrainOutcome::Unknown(reason) => {
                    LinuxDrainStatus::WaitUnknown(reason)
                }
            }
        }
    }
}

fn report_linux_drain(status: LinuxDrainStatus) -> ExitCode {
    match status {
        LinuxDrainStatus::Requested => {
            println!("{}", linux_drain_requested_message());
            ExitCode::SUCCESS
        }
        LinuxDrainStatus::Drained => {
            println!("drained");
            ExitCode::SUCCESS
        }
        LinuxDrainStatus::RequestDeadline => {
            eprintln!("drain_request_deadline_before_mutation");
            ExitCode::from(1)
        }
        LinuxDrainStatus::RequestUnknown => {
            eprintln!("drain_request_unknown_after_mutation; do not retry automatically");
            ExitCode::from(1)
        }
        LinuxDrainStatus::RequestUnavailable => {
            eprintln!("drain_request_unavailable_before_mutation");
            ExitCode::from(1)
        }
        LinuxDrainStatus::WaitDeadline => {
            eprintln!("drain_deadline; admission remains fenced");
            ExitCode::from(1)
        }
        LinuxDrainStatus::WaitUnknown(reason) => {
            eprintln!("{}", linux_drain_unknown_message(reason));
            ExitCode::from(1)
        }
        LinuxDrainStatus::InvalidDeadline => {
            eprintln!(
                "drain timeout must be positive, finite, and within the configured Linux bound"
            );
            ExitCode::from(2)
        }
    }
}

pub(super) fn linux_drain_requested_message() -> &'static str {
    "drain_request_persisted; active_daemon_enforcement_and_quiescence_are_not_proven"
}

pub(super) fn linux_drain_unknown_message(reason: DrainUnknown) -> String {
    let reason = match reason {
        DrainUnknown::StateDirectoryUnavailable => "state_directory_unavailable",
        DrainUnknown::JournalUnavailable => "journal_unavailable",
        DrainUnknown::RuntimeUnavailable => "runtime_unavailable",
        DrainUnknown::AdmissionNotFenced => "admission_not_fenced",
        DrainUnknown::OwnershipInventoryUnavailable => "ownership_inventory_unavailable",
    };
    format!("drain_unknown:{reason}; admission fence state is not proven")
}

pub(super) fn resume(state: &Path, config_path: &Path) -> ExitCode {
    resume_for_os(state, config_path, std::env::consts::OS)
}

pub(super) fn resume_for_os(state: &Path, config_path: &Path, os: &str) -> ExitCode {
    match os {
        "linux" => resume_linux(state, config_path),
        "macos" => remove_flag(state, "drain"),
        _ => {
            eprintln!("resume is unavailable on this platform");
            ExitCode::from(1)
        }
    }
}

fn resume_linux(state: &Path, config_path: &Path) -> ExitCode {
    use velnor_runner_launch::launch::control::{
        ControlOpenError, ResumeBlockReason, ResumeOutcome, resume_blocking,
    };

    let Ok(settings) = linux_drain_settings(config_path) else {
        eprintln!("Linux resume requires a valid protected host configuration");
        return ExitCode::from(1);
    };
    let Some(deadline) =
        Instant::now().checked_add(Duration::from_secs(settings.configured_timeout_secs))
    else {
        eprintln!("configured drain deadline is invalid");
        return ExitCode::from(1);
    };
    match resume_blocking(
        &state.join("launch.db"),
        &settings.docker_endpoint,
        deadline,
    ) {
        Ok(ResumeOutcome::Resumed) => {
            println!("admission_resumed");
            ExitCode::SUCCESS
        }
        Ok(ResumeOutcome::DeadlineBeforeMutation) => {
            eprintln!("resume_deadline_before_mutation; admission remains fenced");
            ExitCode::from(1)
        }
        Ok(ResumeOutcome::UnknownAfterMutation) => {
            eprintln!("resume_unknown_after_mutation; do not retry automatically");
            ExitCode::from(1)
        }
        Ok(ResumeOutcome::Blocked(ResumeBlockReason::QuiescenceProofUnavailable)) => {
            eprintln!("resume_blocked:quiescence_proof_unavailable; admission remains fenced");
            ExitCode::from(1)
        }
        Err(ControlOpenError::JournalUnavailable) => {
            eprintln!("resume_journal_unavailable_before_mutation");
            ExitCode::from(1)
        }
        Err(ControlOpenError::RuntimeUnavailable) => {
            eprintln!("resume_runtime_unavailable_before_mutation");
            ExitCode::from(1)
        }
    }
}

pub(super) fn requested_wait(timeout_secs: Option<u64>) -> String {
    match timeout_secs {
        Some(seconds) => format!("the requested {seconds}-second"),
        None => "the configured-timeout".to_owned(),
    }
}
