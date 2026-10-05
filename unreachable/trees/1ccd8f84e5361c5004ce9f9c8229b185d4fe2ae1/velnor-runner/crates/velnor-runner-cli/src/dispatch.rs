//! Parse and print. The host owns mutation.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use velnor_runner_host::{
    ConnectPlan, DisconnectEffect, HostConfig, HostError, Readiness, SetOwnership, connect_plan,
    disconnect_effects, doctor_json, import_secret, read_secret, readiness_for_empty, status_json,
};

use crate::args::{Cli, Command, DaemonAction};

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
    match &cli.command {
        Command::Status { json } => print_status(&state, *json),
        Command::Doctor { probe } => print_doctor(&state, *probe),
        Command::Logs { follow } => logs(*follow),
        Command::Drain { .. } => flag(&state, "drain"),
        Command::Resume => remove_flag(&state, "drain"),
        Command::Connect {
            repo,
            scale_set,
            platform,
            max_jobs,
            docker_context,
            endpoint,
        } => connect(&ConnectRequest {
            state: &state,
            repo,
            scale_set,
            platform,
            max_jobs: *max_jobs,
            docker_context: docker_context.as_deref(),
            endpoint: endpoint.as_deref(),
        }),
        Command::Service { action } => crate::service::service(*action),
        Command::Daemon { action } => daemon(&state, *action),
        Command::Compare { evidence, .. } => compare_command(evidence.as_deref()),
        Command::Disconnect { drain, .. } => disconnect(*drain),
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
    println!("{}", doctor_json(observe(state), probe));
    ExitCode::SUCCESS
}

fn observe(_state: &Path) -> Readiness {
    readiness_for_empty()
}

fn logs(follow: bool) -> ExitCode {
    let path = crate::service::log_dir().join("host.log");
    if follow && !path.is_file() {
        eprintln!("log missing");
        return ExitCode::from(1);
    }
    println!("{}", path.display());
    ExitCode::SUCCESS
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

pub(crate) const KEYCHAIN_SERVICE: &str = "com.tailrocks.velnor.host";
pub(crate) const KEYCHAIN_ACCOUNT: &str = "velnor-host";

/// Fields for one `connect` invocation. The token is not a field.
struct ConnectRequest<'a> {
    state: &'a Path,
    repo: &'a str,
    scale_set: &'a str,
    platform: &'a str,
    max_jobs: Option<u32>,
    docker_context: Option<&'a str>,
    endpoint: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectError {
    Config,
    Rejected,
    Secret(HostError),
    Write,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config => formatter.write_str("invalid config"),
            Self::Rejected => formatter.write_str("rejected connection"),
            Self::Secret(error) => std::fmt::Display::fmt(error, formatter),
            Self::Write => formatter.write_str("write failed"),
        }
    }
}

fn connect(request: &ConnectRequest<'_>) -> ExitCode {
    let mut stdin = std::io::stdin();
    match connect_with(&mut stdin, KEYCHAIN_SERVICE, request) {
        Ok(()) => {
            println!("{}", status_json(Readiness::WaitingForCredentials));
            ExitCode::SUCCESS
        }
        Err(ConnectError::Rejected) => {
            eprintln!("rejected connection");
            ExitCode::from(1)
        }
        Err(ConnectError::Secret(error)) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
        Err(ConnectError::Config | ConnectError::Write) => ExitCode::from(1),
    }
}

fn connect_with<R: Read>(
    input: &mut R,
    service: &str,
    request: &ConnectRequest<'_>,
) -> Result<(), ConnectError> {
    let text = sample_config(
        request.repo,
        request.scale_set,
        request.platform,
        request.max_jobs.unwrap_or(1),
        request.docker_context,
        request.endpoint,
    );
    let parsed = HostConfig::parse(&text).map_err(|_| ConnectError::Config)?;
    if binding_rejected(request.state, &parsed) {
        return Err(ConnectError::Rejected);
    }
    store_token(input, service)?;
    persist_host(request.state, &text)
}

fn binding_rejected(state: &Path, request: &HostConfig) -> bool {
    let existing = read_host(state);
    connect_plan(existing.as_ref(), request) == ConnectPlan::Rejected
}

fn read_host(state: &Path) -> Option<HostConfig> {
    let raw = std::fs::read_to_string(state.join("host.toml")).ok()?;
    HostConfig::parse(&raw).ok()
}

fn store_token<R: Read>(input: &mut R, service: &str) -> Result<(), ConnectError> {
    let secret = read_secret(input).map_err(ConnectError::Secret)?;
    import_secret(service, KEYCHAIN_ACCOUNT, &secret).map_err(ConnectError::Secret)
}

fn persist_host(state: &Path, text: &str) -> Result<(), ConnectError> {
    if std::fs::create_dir_all(state).is_err()
        || std::fs::write(state.join("host.toml"), text).is_err()
    {
        return Err(ConnectError::Write);
    }
    Ok(())
}

fn sample_config(
    repo: &str,
    scale_set: &str,
    platform: &str,
    max_jobs: u32,
    docker_context: Option<&str>,
    endpoint: Option<&str>,
) -> String {
    let context = docker_context.unwrap_or("orbstack");
    let socket = endpoint.unwrap_or("unix:///var/run/docker.sock");
    format!(
        "schema = 1\n[github]\nrepository = \"{repo}\"\nscale_set_name = \"{scale_set}\"\ncredential_ref = \"keychain:com.tailrocks.velnor.host/local\"\n[host]\nmax_jobs = {max_jobs}\n[docker]\ncontext = \"{context}\"\nplatform = \"{platform}\"\nendpoint = \"{socket}\"\n"
    )
}

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

fn disconnect(drain: bool) -> ExitCode {
    let effects = disconnect_effects(SetOwnership::Adopted, drain);
    if effects.contains(&DisconnectEffect::DeleteSet) {
        eprintln!("refusing to delete an adopted set");
        return ExitCode::from(1);
    }
    println!("disconnected");
    ExitCode::SUCCESS
}
