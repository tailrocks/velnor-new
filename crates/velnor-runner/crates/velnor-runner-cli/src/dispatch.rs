//! Parse and print. The host owns mutation.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};
use std::time::Duration;

use clap::Parser;
use velnor_runner_host::{READINESS_BUDGET, Readiness, doctor_json, status_json};

use crate::args::{Cli, Command, CredentialAction, DaemonAction};

mod connect;
mod connect_authority;
mod connect_config;
mod connection_state;
mod credential;
mod disconnect;
mod drain;
mod marker;

use connect::{ConnectRequest, connect};
use disconnect::disconnect;
use drain::{drain, remove_flag};

/// Parse argv and run one command.
#[must_use]
pub fn run() -> ExitCode {
    if let Some(code) = crate::readiness_probe::run_internal() {
        return code;
    }
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
    match &cli.command {
        Command::Status { json } => print_status(&state, *json),
        Command::Doctor { probe } => print_doctor(&state, *probe),
        Command::Logs { follow } => logs(*follow),
        Command::Drain { wait } => drain(&state, *wait),
        Command::Resume => remove_flag(&state, "drain"),
        Command::Credential { action } => match action {
            CredentialAction::ImportReleaseRead => credential::import_release_read(&state),
        },
        Command::Connect { args } => connect(&ConnectRequest {
            state: &state,
            connection_id: &args.connection_id,
            repo: &args.repo,
            repository_id: args.repository_id,
            job_authority_policy: args.job_authority_policy.as_deref(),
            job_authority_inventory: args.job_authority_inventory.as_deref(),
            job_authority_generator_source_sha: args.job_authority_generator_source_sha.as_deref(),
            job_authority_generator_artifact_sha256: args
                .job_authority_generator_artifact_sha256
                .as_deref(),
            job_authority_generation: args.job_authority_generation,
            scale_set: &args.scale_set,
            scale_set_labels: args.scale_set_labels.clone(),
            source_policy_mode: &args.source_policy_mode,
            events: args.events.clone(),
            branches: args.branches.clone(),
            tag_prefixes: args.tag_prefixes.clone(),
            pull_request_rules: args.pull_request_rules.clone(),
            platform: &args.platform,
            max_jobs: args.max_jobs,
            docker_context: args.docker_context.as_deref(),
            endpoint: &args.endpoint,
            runner_cpu_millicores: args.runner_cpu_millicores,
            runner_memory_bytes: args.runner_memory_bytes,
            dind_cpu_millicores: args.dind_cpu_millicores,
            dind_memory_bytes: args.dind_memory_bytes,
            operational_memory_reserve_bytes: args.operational_memory_reserve_bytes,
        }),
        Command::Service { action } => crate::service::service(*action, &state),
        Command::Daemon { action } => daemon(&state, *action),
        Command::Compare { evidence, .. } => compare_command(evidence.as_deref()),
        Command::Disconnect {
            connection_id,
            wait,
        } => disconnect(&state, connection_id, *wait),
    }
}

fn state_dir(override_path: Option<&Path>) -> PathBuf {
    if let Some(path) = override_path {
        return path.to_path_buf();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    home.map_or_else(
        || PathBuf::from("."),
        |dir| dir.join("Library/Application Support/Velnor"),
    )
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

fn print_doctor(state: &Path, probe: bool) -> ExitCode {
    if probe {
        return match velnor_runner_host::run_doctor_probe(state, Duration::from_secs(240)) {
            Ok(report) => {
                println!("{report}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("doctor probe failed: {error}");
                ExitCode::from(1)
            }
        };
    }
    println!("{}", doctor_json(observe(state), false));
    ExitCode::SUCCESS
}

fn observe(state: &Path) -> Readiness {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build();
    let Ok(runtime) = runtime else {
        return Readiness::Degraded;
    };
    let deadline = tokio::time::Instant::now() + READINESS_BUDGET;
    runtime.block_on(crate::readiness_probe::check(state.to_path_buf(), deadline))
}

fn logs(follow: bool) -> ExitCode {
    let path = crate::service::log_dir().join("host.log");
    if follow {
        if !path.is_file() {
            eprintln!("log missing");
            return ExitCode::from(1);
        }
        let error = ProcessCommand::new("tail").arg("-F").arg(path).exec();
        eprintln!("failed to follow log: {error}");
        return ExitCode::from(1);
    }
    println!("{}", path.display());
    ExitCode::SUCCESS
}

pub(super) const DRAIN_WAIT_BUDGET: Duration = Duration::from_secs(300);

fn daemon(state: &Path, action: DaemonAction) -> ExitCode {
    match action {
        DaemonAction::Run => run_daemon(state),
    }
}

fn run_daemon(state: &Path) -> ExitCode {
    crate::daemon_run::run_daemon(state)
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
mod connect_tests;
#[cfg(test)]
mod credential_tests;
#[cfg(test)]
mod disconnect_tests;
