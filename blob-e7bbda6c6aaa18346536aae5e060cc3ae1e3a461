//! Stop one worker pair on the local Docker engine, or remove one recorded id.
//! Dummy JIT bytes are not printed.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use velnor_runner_host::{
    DeleteDecision, HostConfig, PairStop, connect_unix, remove_recorded, start_pair_until,
};

const DUMMY_JIT: &[u8] = b"stage-stop-not-a-jit";

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    if first == "remove" {
        return remove_owned(args.next(), args.next()).await;
    }
    stage(&first, args.next()).await
}

fn usage() {
    eprintln!("usage: stage_once <stop> <volume>");
    eprintln!("       stage_once remove <owned-id> <name>");
}

async fn stage(stop_name: &str, volume: Option<String>) -> ExitCode {
    let Some(volume) = volume else {
        usage();
        return ExitCode::from(2);
    };
    let Some(stop) = parse_stop(stop_name) else {
        eprintln!("unknown stop");
        return ExitCode::from(2);
    };
    let docker = match open_docker() {
        Ok(docker) => docker,
        Err(code) => return code,
    };
    let resource_budget = match host_resource_budget() {
        Ok(budget) => budget,
        Err(code) => return code,
    };
    match start_pair_until(&docker, &volume, resource_budget, DUMMY_JIT, stop).await {
        Ok(partial) => {
            let dind = partial.dind_id.as_deref().unwrap_or("");
            let runner = partial.runner_id.as_deref().unwrap_or("");
            println!("stop={stop_name} dind_id={dind} runner_id={runner}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("stage_once: {err}");
            ExitCode::from(1)
        }
    }
}

fn host_resource_budget() -> Result<velnor_runner_host::ResourceBudget, ExitCode> {
    let home = std::env::var("HOME").map_err(|_| ExitCode::from(1))?;
    let path = PathBuf::from(home).join("Library/Application Support/Velnor/host.toml");
    let text = std::fs::read_to_string(path).map_err(|_| ExitCode::from(1))?;
    let config = HostConfig::parse(&text).map_err(|_| ExitCode::from(1))?;
    config.resource_budget().map_err(|_| ExitCode::from(1))
}

async fn remove_owned(owned: Option<String>, name: Option<String>) -> ExitCode {
    let (Some(owned), Some(name)) = (owned, name) else {
        usage();
        return ExitCode::from(2);
    };
    let docker = match open_docker() {
        Ok(docker) => docker,
        Err(code) => return code,
    };
    match remove_recorded(&docker, &owned, &name).await {
        Ok(decision) => {
            println!(
                "decision={} owned_id={owned} name={name}",
                decision_name(decision)
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("stage_once: {err}");
            ExitCode::from(1)
        }
    }
}

fn parse_stop(name: &str) -> Option<PairStop> {
    match name {
        "volumes" => Some(PairStop::Volumes),
        "dind-created" => Some(PairStop::DindCreated),
        "dind-started" => Some(PairStop::DindStarted),
        "runner-created" => Some(PairStop::RunnerCreated),
        "runner-started" => Some(PairStop::RunnerStarted),
        "jit" => Some(PairStop::Jit),
        _ => None,
    }
}

const fn decision_name(decision: DeleteDecision) -> &'static str {
    match decision {
        DeleteDecision::Delete => "Delete",
        DeleteDecision::KeepForeign => "KeepForeign",
        DeleteDecision::NotDeleted => "NotDeleted",
    }
}

fn open_docker() -> Result<bollard::Docker, ExitCode> {
    connect_unix(&docker_socket()?).map_err(|err| {
        eprintln!("stage_once: {err}");
        ExitCode::from(1)
    })
}

fn docker_socket() -> Result<String, ExitCode> {
    if let Ok(host) = std::env::var("DOCKER_HOST")
        && host.starts_with("unix://")
    {
        return Ok(host);
    }
    let output = Command::new("docker")
        .args([
            "context",
            "inspect",
            "--format",
            "{{.Endpoints.docker.Host}}",
        ])
        .output()
        .map_err(|_| ExitCode::from(1))?;
    if !output.status.success() {
        return Err(ExitCode::from(1));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| ExitCode::from(1))?;
    let host = text.trim();
    if host.starts_with("unix://") {
        Ok(host.to_owned())
    } else {
        Err(ExitCode::from(1))
    }
}
