//! Parse and print. The host owns mutation.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::Parser;
use velnor_runner_host::docker_client::{self, DockerVersion};
use velnor_runner_host::{
    DisconnectEffect, HostConfig, HostError, HostPlatform, Readiness, SetOwnership,
    disconnect_effects, doctor_json, load_configured_secret, read_host_config_file,
    readiness_for_empty,
};
use velnor_runner_launch::launch::control::DrainUnknown;

use crate::args::{Cli, Command, DaemonAction};

mod connect;

/// Parse argv and run one command.
#[must_use]
pub fn run() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => dispatch(&cli),
        Err(error) => {
            let code = error.exit_code();
            if error.print().is_err() {
                return ExitCode::from(2);
            }
            u8::try_from(code).map_or(ExitCode::from(2), ExitCode::from)
        }
    }
}

fn dispatch(cli: &Cli) -> ExitCode {
    let state = state_dir(cli.state.as_deref());
    let config = selected_config_path(cli, &state);
    match &cli.command {
        Command::Status { json } => print_status(&state, &config, *json),
        Command::Doctor { probe } => print_doctor(&state, &config, *probe),
        Command::Logs { follow } => crate::service::logs(*follow),
        Command::Drain { wait, timeout_secs } => drain(&state, &config, *wait, *timeout_secs),
        Command::Resume => resume(&state, &config),
        Command::Connect(request) => connect::connect(&connect::ConnectRequest {
            config_path: &config,
            repo: &request.repo,
            scale_set: &request.scale_set,
            platform: &request.platform,
            host_platform: request.host_platform.as_deref(),
            registration_scope: request.registration_scope.as_deref(),
            runner_group_id: request.runner_group_id,
            runner_group_name: request.runner_group_name.as_deref(),
            allowed_events: &request.allowed_events,
            allowed_workflow_paths: &request.allowed_workflow_paths,
            image_profile: request.image_profile.as_deref(),
            max_jobs: request.max_jobs,
            drain_timeout_secs: request.drain_timeout_secs,
            docker_context: request.docker_context.as_deref(),
            endpoint: request.endpoint.as_deref(),
        }),
        Command::Service { action } => crate::service::service(*action, &config, &state),
        Command::Daemon { action } => daemon(&state, &config, *action),
        Command::Compare {
            repo,
            run_id,
            attempt,
            evidence,
        } => compare_command(repo, *run_id, *attempt, evidence.as_deref()),
        Command::Disconnect {
            drain,
            wait,
            timeout_secs,
        } => disconnect(&state, *drain, *wait, *timeout_secs),
    }
}

fn state_dir(override_path: Option<&Path>) -> PathBuf {
    if let Some(path) = override_path {
        return path.to_path_buf();
    }
    #[cfg(target_os = "linux")]
    {
        PathBuf::from("/var/lib/velnor-host")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        home.map_or_else(
            || PathBuf::from("."),
            |dir| dir.join("Library/Application Support/Velnor"),
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        PathBuf::new()
    }
}

fn config_path(override_path: Option<&Path>, state: &Path) -> PathBuf {
    if let Some(path) = override_path {
        return path.to_path_buf();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = state;
        PathBuf::from(velnor_runner_host::LINUX_CONFIG_PATH)
    }
    #[cfg(target_os = "macos")]
    {
        state.join("host.toml")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = state;
        PathBuf::new()
    }
}

fn selected_config_path(cli: &Cli, state: &Path) -> PathBuf {
    config_path(cli.config.as_deref(), state)
}

fn print_status(state: &Path, config_path: &Path, json: bool) -> ExitCode {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let platform = host_platform();
        let observation = status_observation_with(
            read_host_config_file(config_path, platform),
            platform,
            journal_file_observation(state),
            crate::service::controller_service_state(),
            |reference| {
                load_configured_secret(reference)
                    .map(|secret| credential_is_available(secret.as_slice()))
            },
            docker_client::read_version_blocking,
        );
        if json {
            println!("{}", observation.json());
        } else {
            println!("{}", observation.lines());
        }
        ExitCode::SUCCESS
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (state, config_path);
        let observation = StatusObservation {
            config: ConfigObservation::Unavailable,
            credential: DependencyObservation::NotChecked,
            docker: DependencyObservation::NotChecked,
            journal: JournalObservation::Unknown,
            controller_service: crate::service::ControllerServiceState::Unknown,
        };
        if json {
            println!("{}", observation.json());
        } else {
            println!("{}", observation.lines());
        }
        ExitCode::SUCCESS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigObservation {
    Valid,
    Missing,
    Invalid,
    Unavailable,
}

impl ConfigObservation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencyObservation {
    Available,
    Unavailable,
    NotChecked,
}

impl DependencyObservation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::NotChecked => "not_checked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalObservation {
    PresentUnverified,
    Missing,
    Unsafe,
    Unknown,
}

impl JournalObservation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PresentUnverified => "present_unverified",
            Self::Missing => "missing",
            Self::Unsafe => "unsafe",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatusObservation {
    config: ConfigObservation,
    credential: DependencyObservation,
    docker: DependencyObservation,
    journal: JournalObservation,
    controller_service: crate::service::ControllerServiceState,
}

impl StatusObservation {
    fn json(self) -> String {
        serde_json::json!({
            "command": "status",
            "state": "not_proven",
            "config": self.config.as_str(),
            "credential": self.credential.as_str(),
            "docker": self.docker.as_str(),
            "journal": self.journal.as_str(),
            "controller_service": controller_service_status(self.controller_service),
            "global_readiness": "not_proven",
        })
        .to_string()
    }

    fn lines(self) -> String {
        format!(
            "state=not_proven\nconfig={}\ncredential={}\ndocker={}\njournal={}\ncontroller_service={}\nglobal_readiness=not_proven",
            self.config.as_str(),
            self.credential.as_str(),
            self.docker.as_str(),
            self.journal.as_str(),
            controller_service_status(self.controller_service),
        )
    }
}

fn controller_service_status(state: crate::service::ControllerServiceState) -> &'static str {
    match state {
        crate::service::ControllerServiceState::InUse => "in_use",
        crate::service::ControllerServiceState::Stopped => "stopped_or_absent",
        crate::service::ControllerServiceState::Unknown => "unknown",
    }
}

fn status_observation_with(
    config_text: Result<Option<String>, HostError>,
    platform: HostPlatform,
    journal: JournalObservation,
    controller_service: crate::service::ControllerServiceState,
    credential_probe: impl FnOnce(&str) -> Result<bool, HostError>,
    docker_probe: impl FnOnce(&str) -> Result<DockerVersion, HostError>,
) -> StatusObservation {
    let mut observation = StatusObservation {
        config: ConfigObservation::Unavailable,
        credential: DependencyObservation::NotChecked,
        docker: DependencyObservation::NotChecked,
        journal,
        controller_service,
    };
    let text = match config_text {
        Ok(Some(text)) => text,
        Ok(None) => {
            observation.config = ConfigObservation::Missing;
            return observation;
        }
        Err(_) => return observation,
    };
    let Ok(config) = HostConfig::parse(&text) else {
        observation.config = ConfigObservation::Invalid;
        return observation;
    };
    if config.validate_for_host(platform).is_err() {
        observation.config = ConfigObservation::Invalid;
        return observation;
    }
    observation.config = ConfigObservation::Valid;
    observation.credential = match credential_probe(&config.github.credential_ref) {
        Ok(true) => DependencyObservation::Available,
        Ok(false) | Err(_) => DependencyObservation::Unavailable,
    };
    observation.docker = match docker_probe(&config.docker.endpoint) {
        Ok(_) => DependencyObservation::Available,
        Err(_) => DependencyObservation::Unavailable,
    };
    observation
}

fn credential_is_available(secret: &[u8]) -> bool {
    std::str::from_utf8(secret).is_ok_and(|value| !value.trim().is_empty())
}

fn journal_file_observation(state: &Path) -> JournalObservation {
    match std::fs::symlink_metadata(state.join("launch.db")) {
        Ok(metadata) if metadata.file_type().is_file() => JournalObservation::PresentUnverified,
        Ok(_) => JournalObservation::Unsafe,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => JournalObservation::Missing,
        Err(_) => JournalObservation::Unknown,
    }
}

fn print_doctor(state: &Path, config_path: &Path, probe: bool) -> ExitCode {
    if !probe {
        println!("{}", doctor_json(observe(state), false));
        return ExitCode::SUCCESS;
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let result = {
        let platform = host_platform();
        read_host_config_file(config_path, platform)
            .map_err(|_| DoctorProbeFailure::Config)
            .and_then(|config| config.ok_or(DoctorProbeFailure::Config))
            .and_then(|text| {
                probe_config_text(&text, platform, docker_client::read_version_blocking)
            })
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let result = Err(DoctorProbeFailure::Config);
    println!("{}", doctor_probe_document(result.clone()));
    if result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(target_os = "linux")]
const fn host_platform() -> HostPlatform {
    HostPlatform::Linux
}

#[cfg(target_os = "macos")]
const fn host_platform() -> HostPlatform {
    HostPlatform::Macos
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorProbeFailure {
    Config,
    Docker,
}

fn probe_config_text(
    text: &str,
    platform: HostPlatform,
    probe: impl FnOnce(&str) -> Result<DockerVersion, HostError>,
) -> Result<DockerVersion, DoctorProbeFailure> {
    let config = HostConfig::parse(text).map_err(|_| DoctorProbeFailure::Config)?;
    config
        .validate_for_host(platform)
        .map_err(|_| DoctorProbeFailure::Config)?;
    probe(&config.docker.endpoint).map_err(|_| DoctorProbeFailure::Docker)
}

fn doctor_probe_document(result: Result<DockerVersion, DoctorProbeFailure>) -> serde_json::Value {
    match result {
        Ok(version) => serde_json::json!({
            "command": "doctor",
            "docker": {
                "status": "available",
                "server_version": version.server_version,
                "api_version": version.api_version,
                "os": version.os,
                "architecture": version.architecture,
            },
            "global_readiness": "not_proven",
        }),
        Err(failure) => serde_json::json!({
            "command": "doctor",
            "docker": { "status": "unavailable" },
            "failure": match failure {
                DoctorProbeFailure::Config => "invalid_or_unavailable_config",
                DoctorProbeFailure::Docker => "docker_version_probe_failed",
            },
            "global_readiness": "not_proven",
        }),
    }
}

fn observe(_state: &Path) -> Readiness {
    readiness_for_empty()
}

fn write_flag_marker(state: &Path, name: &str) -> bool {
    std::fs::create_dir_all(state).is_ok() && std::fs::write(state.join(name), b"1").is_ok()
}

fn remove_flag(state: &Path, name: &str) -> ExitCode {
    let path = state.join(name);
    if path.exists() && std::fs::remove_file(path).is_err() {
        return ExitCode::from(1);
    }
    println!("ready_for_admission");
    ExitCode::SUCCESS
}

fn drain(state: &Path, config_path: &Path, wait: bool, timeout_secs: Option<u64>) -> ExitCode {
    drain_for_os(state, config_path, wait, timeout_secs, std::env::consts::OS)
}

fn drain_for_os(
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
                eprintln!("legacy drain marker recorded; macOS drain wait is not proven");
                ExitCode::from(1)
            } else {
                println!("draining");
                ExitCode::SUCCESS
            }
        }
        _ => {
            eprintln!("drain is unavailable on this platform");
            ExitCode::from(1)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinuxDrainSettings {
    docker_endpoint: String,
    configured_timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxDrainStatus {
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
    let status = linux_drain_with(
        &state.join("launch.db"),
        wait,
        timeout_secs,
        &settings,
        velnor_runner_launch::launch::control::request_drain_blocking,
        velnor_runner_launch::launch::control::wait_drained_blocking,
    );
    report_linux_drain(status)
}

fn linux_drain_with<R, W>(
    journal_path: &Path,
    wait: bool,
    timeout_secs: Option<u64>,
    settings: &LinuxDrainSettings,
    request: R,
    wait_drained: W,
) -> LinuxDrainStatus
where
    R: FnOnce(
        &Path,
        Instant,
    ) -> Result<
        velnor_runner_launch::launch::control::DrainRequestOutcome,
        velnor_runner_launch::launch::control::ControlOpenError,
    >,
    W: FnOnce(&Path, &str, Instant) -> velnor_runner_launch::launch::control::DrainOutcome,
{
    let seconds = timeout_secs.unwrap_or(settings.configured_timeout_secs);
    let Some(deadline) = (seconds > 0)
        .then(|| Instant::now().checked_add(Duration::from_secs(seconds)))
        .flatten()
    else {
        return LinuxDrainStatus::InvalidDeadline;
    };
    let request_result = request(journal_path, deadline);
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
            match wait_drained(journal_path, &settings.docker_endpoint, deadline) {
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
            println!("drain_requested");
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
            eprintln!("drain timeout must be positive and finite");
            ExitCode::from(2)
        }
    }
}

fn linux_drain_unknown_message(reason: DrainUnknown) -> String {
    let reason = match reason {
        DrainUnknown::JournalUnavailable => "journal_unavailable",
        DrainUnknown::RuntimeUnavailable => "runtime_unavailable",
        DrainUnknown::AdmissionNotFenced => "admission_not_fenced",
        DrainUnknown::OwnershipInventoryUnavailable => "ownership_inventory_unavailable",
    };
    format!("drain_unknown:{reason}; admission fence state is not proven")
}

fn resume(state: &Path, config_path: &Path) -> ExitCode {
    resume_for_os(state, config_path, std::env::consts::OS)
}

fn resume_for_os(state: &Path, config_path: &Path, os: &str) -> ExitCode {
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

fn daemon(state: &Path, config: &Path, action: DaemonAction) -> ExitCode {
    match action {
        DaemonAction::Run => run_daemon(state, config),
    }
}

fn run_daemon(state: &Path, config: &Path) -> ExitCode {
    crate::daemon_run::run_daemon(state, config)
}

fn compare_command(
    repository: &str,
    run_id: u64,
    attempt: u64,
    evidence: Option<&Path>,
) -> ExitCode {
    match evidence {
        Some(path) => crate::compare::compare_dir_for(path, repository, run_id, attempt),
        None => not_proven(),
    }
}

fn not_proven() -> ExitCode {
    println!("NOT_PROVEN");
    ExitCode::from(1)
}

#[cfg(test)]
mod tests;

fn disconnect(state: &Path, drain: bool, wait: bool, timeout_secs: Option<u64>) -> ExitCode {
    disconnect_for_os(state, drain, wait, timeout_secs, std::env::consts::OS)
}

fn disconnect_for_os(
    state: &Path,
    drain: bool,
    wait: bool,
    timeout_secs: Option<u64>,
    os: &str,
) -> ExitCode {
    if !drain || !wait {
        eprintln!("disconnect requires --drain --wait");
        return ExitCode::from(2);
    }

    let effects = disconnect_effects(SetOwnership::Adopted, true);
    if effects.contains(&DisconnectEffect::DeleteSet) {
        eprintln!("refusing to delete an adopted set");
        return ExitCode::from(1);
    }

    if os == "macos" {
        if !write_flag_marker(state, "drain") {
            eprintln!("failed to record the legacy drain marker");
            return ExitCode::from(1);
        }
        eprintln!(
            "legacy drain marker recorded; {} drain wait, physical quiescence, and remote scale-set disconnection are not proven",
            requested_wait(timeout_secs)
        );
    } else {
        eprintln!(
            "{} disconnect is unavailable; {} drain wait and remote scale-set disconnection are not proven",
            os,
            requested_wait(timeout_secs)
        );
    }
    ExitCode::from(1)
}

fn requested_wait(timeout_secs: Option<u64>) -> String {
    match timeout_secs {
        Some(seconds) => format!("the requested {seconds}-second"),
        None => "the configured-timeout".to_owned(),
    }
}
