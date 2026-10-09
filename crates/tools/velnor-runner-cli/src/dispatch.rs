//! Command routing and platform-neutral path selection.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use crate::args::{Cli, Command, DaemonAction};

mod connect;
mod disconnect;
mod drain;
mod readiness;
mod status;

use self::disconnect::disconnect;
use self::drain::{drain, resume};
use self::status::{print_doctor, print_status};

#[cfg(test)]
use self::drain::{
    LinuxDrainSettings, LinuxDrainStatus, drain_for_os, linux_drain_requested_message,
    linux_drain_unknown_message, linux_drain_with, resume_for_os,
};
#[cfg(test)]
use self::status::{
    ConfigObservation, DependencyObservation, DoctorProbeFailure, JournalObservation,
    credential_is_available, doctor_probe_document, journal_file_observation, probe_config_text,
    status_observation_with,
};

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
            trust_policy_file: request.trust_policy_file.clone(),
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
            scoped_evidence,
        } => compare_command(
            repo,
            *run_id,
            *attempt,
            evidence.as_deref(),
            scoped_evidence.as_deref(),
        ),
        Command::Disconnect {
            drain: should_drain,
            wait,
            timeout_secs,
            remove_local,
        } => disconnect(
            &state,
            &config,
            *should_drain,
            *wait,
            *timeout_secs,
            *remove_local,
        ),
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

fn daemon(state: &Path, config: &Path, action: DaemonAction) -> ExitCode {
    match action {
        DaemonAction::Run => crate::daemon_run::run_daemon(state, config),
    }
}

fn compare_command(
    repository: &str,
    run_id: u64,
    attempt: u64,
    evidence: Option<&Path>,
    scoped_evidence: Option<&Path>,
) -> ExitCode {
    match (evidence, scoped_evidence) {
        (Some(path), None) => crate::compare::compare_dir_for(path, repository, run_id, attempt),
        (None, Some(path)) => {
            crate::compare::compare_scoped_file_for(path, repository, run_id, attempt)
        }
        _ => not_proven(),
    }
}

fn not_proven() -> ExitCode {
    println!("NOT_PROVEN");
    ExitCode::from(1)
}

#[cfg(test)]
mod tests;
