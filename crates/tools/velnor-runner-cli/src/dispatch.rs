//! Parse and print. The host owns mutation.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use velnor_runner_host::docker_client::{self, DockerVersion};
use velnor_runner_host::{
    DisconnectEffect, HostConfig, HostError, HostPlatform, Readiness, SetOwnership,
    disconnect_effects, doctor_json, read_host_config_file, readiness_for_empty, status_json,
};

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
        Command::Status { json } => print_status(&state, *json),
        Command::Doctor { probe } => print_doctor(&state, &config, *probe),
        Command::Logs { follow } => crate::service::logs(*follow),
        Command::Drain { .. } => flag(&state, "drain"),
        Command::Resume => remove_flag(&state, "drain"),
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
        Command::Service { action } => crate::service::service(*action),
        Command::Daemon { action } => daemon(&state, &config, *action),
        Command::Compare { evidence, .. } => compare_command(evidence.as_deref()),
        Command::Disconnect { drain, .. } => disconnect(*drain),
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

fn print_status(state: &Path, json: bool) -> ExitCode {
    let readiness = observe(state);
    if json {
        println!("{}", status_json(readiness));
    } else {
        println!("{}", readiness.as_str());
    }
    ExitCode::SUCCESS
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

fn flag(state: &Path, name: &str) -> ExitCode {
    if std::fs::create_dir_all(state).is_err() {
        return ExitCode::from(1);
    }
    if std::fs::write(state.join(name), b"1").is_err() {
        return ExitCode::from(1);
    }
    println!("draining");
    ExitCode::SUCCESS
}

fn remove_flag(state: &Path, name: &str) -> ExitCode {
    let path = state.join(name);
    if path.exists() && std::fs::remove_file(path).is_err() {
        return ExitCode::from(1);
    }
    println!("ready_for_admission");
    ExitCode::SUCCESS
}

fn daemon(state: &Path, config: &Path, action: DaemonAction) -> ExitCode {
    match action {
        DaemonAction::Run => run_daemon(state, config),
    }
}

fn run_daemon(state: &Path, config: &Path) -> ExitCode {
    crate::daemon_run::run_daemon(state, config)
}

fn compare_command(evidence: Option<&Path>) -> ExitCode {
    match evidence {
        Some(path) => crate::compare::compare_dir(path),
        None => not_proven(),
    }
}

fn not_proven() -> ExitCode {
    println!("NOT_PROVEN");
    ExitCode::from(1)
}

#[cfg(test)]
mod tests;

fn disconnect(drain: bool) -> ExitCode {
    let effects = disconnect_effects(SetOwnership::Adopted, drain);
    if effects.contains(&DisconnectEffect::DeleteSet) {
        eprintln!("refusing to delete an adopted set");
        return ExitCode::from(1);
    }
    println!("disconnected");
    ExitCode::SUCCESS
}
