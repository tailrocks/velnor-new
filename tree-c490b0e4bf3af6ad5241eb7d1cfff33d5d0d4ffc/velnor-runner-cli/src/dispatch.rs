//! Parse and print. The host owns mutation.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use velnor_runner_host::{
    ConnectPlan, DisconnectEffect, HostConfig, HostError, KeychainReference, READINESS_BUDGET,
    Readiness, SetOwnership, connect_plan, disconnect_effects, doctor_json, import_secret,
    read_secret, status_json,
};

use crate::args::{Cli, Command, DaemonAction};

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
        Command::Drain { .. } => flag(&state, "drain"),
        Command::Resume => remove_flag(&state, "drain"),
        Command::Connect {
            repo,
            scale_set,
            platform,
            max_jobs,
            docker_context,
            endpoint,
            runner_cpu_millicores,
            runner_memory_bytes,
            dind_cpu_millicores,
            dind_memory_bytes,
        } => connect(&ConnectRequest {
            state: &state,
            repo,
            scale_set,
            platform,
            max_jobs: *max_jobs,
            docker_context: docker_context.as_deref(),
            endpoint: endpoint.as_deref(),
            runner_cpu_millicores: *runner_cpu_millicores,
            runner_memory_bytes: *runner_memory_bytes,
            dind_cpu_millicores: *dind_cpu_millicores,
            dind_memory_bytes: *dind_memory_bytes,
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
const DEFAULT_KEYCHAIN_ACCOUNT: &str = "local";

/// Fields for one `connect` invocation. The token is not a field.
struct ConnectRequest<'a> {
    state: &'a Path,
    repo: &'a str,
    scale_set: &'a str,
    platform: &'a str,
    max_jobs: Option<u32>,
    docker_context: Option<&'a str>,
    endpoint: Option<&'a str>,
    runner_cpu_millicores: u64,
    runner_memory_bytes: u64,
    dind_cpu_millicores: u64,
    dind_memory_bytes: u64,
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
    connect_with_store(input, service, request, import_secret)
}

fn connect_with_store<R, F>(
    input: &mut R,
    service: &str,
    request: &ConnectRequest<'_>,
    store: F,
) -> Result<(), ConnectError>
where
    R: Read,
    F: FnOnce(&str, &str, &[u8]) -> Result<(), HostError>,
{
    let default_reference =
        KeychainReference::parse(&format!("keychain:{service}/{DEFAULT_KEYCHAIN_ACCOUNT}"))
            .map_err(|_| ConnectError::Config)?;
    let params = SampleConfig {
        repo: request.repo,
        scale_set: request.scale_set,
        platform: request.platform,
        max_jobs: request.max_jobs.unwrap_or(1),
        docker_context: request.docker_context,
        endpoint: request.endpoint,
        runner_cpu_millicores: request.runner_cpu_millicores,
        runner_memory_bytes: request.runner_memory_bytes,
        dind_cpu_millicores: request.dind_cpu_millicores,
        dind_memory_bytes: request.dind_memory_bytes,
    };
    let candidate_text = sample_config(&params, &default_reference);
    let candidate = HostConfig::parse(&candidate_text).map_err(|_| ConnectError::Config)?;
    let existing = read_host(request.state);
    if connect_plan(existing.as_ref(), &candidate) == ConnectPlan::Rejected {
        return Err(ConnectError::Rejected);
    }
    let credential_ref = existing
        .map(|config| config.github.credential_ref)
        .unwrap_or(default_reference);
    let text = sample_config(&params, &credential_ref);
    store_token(input, &credential_ref, store)?;
    persist_host(request.state, &text)
}

fn read_host(state: &Path) -> Option<HostConfig> {
    let raw = std::fs::read_to_string(state.join("host.toml")).ok()?;
    HostConfig::parse(&raw).ok()
}

fn store_token<R, F>(
    input: &mut R,
    credential_ref: &KeychainReference,
    store: F,
) -> Result<(), ConnectError>
where
    R: Read,
    F: FnOnce(&str, &str, &[u8]) -> Result<(), HostError>,
{
    let secret = read_secret(input).map_err(ConnectError::Secret)?;
    store(credential_ref.service(), credential_ref.account(), &secret).map_err(ConnectError::Secret)
}

fn persist_host(state: &Path, text: &str) -> Result<(), ConnectError> {
    if std::fs::create_dir_all(state).is_err()
        || std::fs::write(state.join("host.toml"), text).is_err()
    {
        return Err(ConnectError::Write);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct SampleConfig<'a> {
    repo: &'a str,
    scale_set: &'a str,
    platform: &'a str,
    max_jobs: u32,
    docker_context: Option<&'a str>,
    endpoint: Option<&'a str>,
    runner_cpu_millicores: u64,
    runner_memory_bytes: u64,
    dind_cpu_millicores: u64,
    dind_memory_bytes: u64,
}

fn sample_config(params: &SampleConfig<'_>, credential_ref: &KeychainReference) -> String {
    let context = params.docker_context.unwrap_or("orbstack");
    let socket = params.endpoint.unwrap_or("unix:///var/run/docker.sock");
    let credential_ref = toml_basic_string(&credential_ref.to_string());
    let SampleConfig {
        repo,
        scale_set,
        platform,
        max_jobs,
        runner_cpu_millicores,
        runner_memory_bytes,
        dind_cpu_millicores,
        dind_memory_bytes,
        ..
    } = *params;
    format!(
        "schema = 1\n[github]\nrepository = \"{repo}\"\nscale_set_name = \"{scale_set}\"\ncredential_ref = {credential_ref}\n[host]\nmax_jobs = {max_jobs}\n[host.resources]\nrunner_cpu_millicores = {runner_cpu_millicores}\nrunner_memory_bytes = {runner_memory_bytes}\ndind_cpu_millicores = {dind_cpu_millicores}\ndind_memory_bytes = {dind_memory_bytes}\n[docker]\ncontext = \"{context}\"\nplatform = \"{platform}\"\nendpoint = \"{socket}\"\n"
    )
}

fn toml_basic_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
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
