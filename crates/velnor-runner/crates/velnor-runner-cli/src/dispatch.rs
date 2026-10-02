//! Parse and print. The host owns mutation.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use velnor_runner_host::{
    ConnectPlan, DaemonLock, DisconnectEffect, HostConfig, Readiness, SetOwnership, connect_plan,
    disconnect_effects, doctor_json, readiness_for_empty, status_json,
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
        } => connect(
            &state,
            repo,
            scale_set,
            platform,
            *max_jobs,
            docker_context.as_deref(),
            endpoint.as_deref(),
        ),
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

fn connect(
    state: &Path,
    repo: &str,
    scale_set: &str,
    platform: &str,
    max_jobs: Option<u32>,
    docker_context: Option<&str>,
    endpoint: Option<&str>,
) -> ExitCode {
    let text = sample_config(
        repo,
        scale_set,
        platform,
        max_jobs.unwrap_or(1),
        docker_context,
        endpoint,
    );
    let Ok(request) = HostConfig::parse(&text) else {
        return ExitCode::from(1);
    };
    let existing = std::fs::read_to_string(state.join("host.toml"))
        .ok()
        .and_then(|raw| HostConfig::parse(&raw).ok());
    if connect_plan(existing.as_ref(), &request) == ConnectPlan::Rejected {
        eprintln!("rejected connection");
        return ExitCode::from(1);
    }
    if std::fs::create_dir_all(state).is_err()
        || std::fs::write(state.join("host.toml"), text).is_err()
    {
        return ExitCode::from(1);
    }
    println!("{}", status_json(Readiness::WaitingForCredentials));
    ExitCode::SUCCESS
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
        DaemonAction::Run => match DaemonLock::try_acquire(&state.join("daemon.lock")) {
            Ok(lock) => hold(&lock),
            Err(_) => ExitCode::from(1),
        },
    }
}

fn hold(lock: &DaemonLock) -> ExitCode {
    if !lock.is_held() {
        return ExitCode::from(1);
    }
    std::thread::park();
    ExitCode::SUCCESS
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

fn disconnect(drain: bool) -> ExitCode {
    let effects = disconnect_effects(SetOwnership::Adopted, drain);
    if effects.contains(&DisconnectEffect::DeleteSet) {
        eprintln!("refusing to delete an adopted set");
        return ExitCode::from(1);
    }
    println!("disconnected");
    ExitCode::SUCCESS
}
