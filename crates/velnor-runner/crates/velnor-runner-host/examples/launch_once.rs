//! Acquire one scale-set job, mint JIT, and start the official worker.
//! The PAT comes from `gh auth token` and is not printed. JIT is not printed.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use velnor_runner_host::{Journal, connect_unix, launch_once};
use zeroize::Zeroize;

#[tokio::main]
async fn main() -> ExitCode {
    let mut pat = match read_pat() {
        Ok(pat) => pat,
        Err(code) => return code,
    };
    let outcome = run(&pat).await;
    pat.zeroize();
    outcome
}

async fn run(pat: &str) -> ExitCode {
    let socket = match docker_socket() {
        Ok(socket) => socket,
        Err(code) => return code,
    };
    let docker = match connect_unix(&socket) {
        Ok(docker) => docker,
        Err(err) => {
            eprintln!("launch_once: {err}");
            return ExitCode::from(1);
        }
    };
    let path = match journal_path() {
        Ok(path) => path,
        Err(code) => return code,
    };
    let journal = match Journal::open(&path).await {
        Ok(journal) => journal,
        Err(err) => {
            eprintln!("launch_once: {err}");
            return ExitCode::from(1);
        }
    };
    match launch_once(pat, "tailrocks", "velnor-new", &docker, &journal).await {
        Ok(report) => {
            let runner = report
                .started
                .as_ref()
                .map_or("", |started| started.runner_id.as_str());
            let dind = report
                .started
                .as_ref()
                .map_or("", |started| started.dind_id.as_str());
            println!(
                "set_id={} started={} runner_id={} dind_id={}",
                report.set_id,
                report.started.is_some(),
                runner,
                dind
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("launch_once: {err}");
            ExitCode::from(1)
        }
    }
}

fn journal_path() -> Result<PathBuf, ExitCode> {
    let home = std::env::var("HOME").map_err(|_| ExitCode::from(1))?;
    let dir = PathBuf::from(home).join("Library/Application Support/Velnor");
    std::fs::create_dir_all(&dir).map_err(|_| ExitCode::from(1))?;
    Ok(dir.join("launch.db"))
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

fn read_pat() -> Result<String, ExitCode> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .map_err(|_| ExitCode::from(1))?;
    if !output.status.success() {
        return Err(ExitCode::from(1));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| ExitCode::from(1))?;
    let pat = text.trim().to_owned();
    if pat.is_empty() {
        Err(ExitCode::from(1))
    } else {
        Ok(pat)
    }
}
