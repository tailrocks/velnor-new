//! Parse and print. The host owns mutation.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use velnor_runner_host::{
    ConnectPlan, DaemonLock, DisconnectEffect, HostConfig, Readiness, SetOwnership, connect_plan,
    disconnect_effects, doctor_json, launch_agent_plist, readiness_for_empty, status_json,
};

use crate::args::{Cli, Command, DaemonAction, ServiceAction};

/// Parse argv and run one command.
#[must_use]
pub fn run() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => dispatch(&cli),
        Err(error) => {
            if error.print().is_err() {
                return ExitCode::from(2);
            }
            ExitCode::from(2)
        }
    }
}

fn dispatch(cli: &Cli) -> ExitCode {
    let state = state_dir(cli.state.as_deref());
    match &cli.command {
        Command::Status { json } => print_status(&state, *json),
        Command::Doctor { probe } => print_doctor(&state, *probe),
        Command::Logs { follow } => logs(&state, *follow),
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
        Command::Service { action } => service(&state, *action),
        Command::Daemon { action } => daemon(&state, *action),
        Command::Compare { .. } => not_proven(),
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

fn logs(state: &Path, follow: bool) -> ExitCode {
    let path = state.join("host.log");
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

fn service(state: &Path, action: ServiceAction) -> ExitCode {
    match action {
        ServiceAction::Install => install(state),
        ServiceAction::Start | ServiceAction::Stop | ServiceAction::Uninstall => {
            println!("{}", action_name(action));
            ExitCode::SUCCESS
        }
    }
}

fn action_name(action: ServiceAction) -> &'static str {
    match action {
        ServiceAction::Install => "install",
        ServiceAction::Start => "start",
        ServiceAction::Stop => "stop",
        ServiceAction::Uninstall => "uninstall",
    }
}

fn install(state: &Path) -> ExitCode {
    let Ok(bin) = std::env::current_exe() else {
        return ExitCode::from(1);
    };
    let Ok(plist) = launch_agent_plist(&bin) else {
        return ExitCode::from(1);
    };
    if std::fs::create_dir_all(state).is_err()
        || std::fs::write(state.join("host.plist"), plist).is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
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
