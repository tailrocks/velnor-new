// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use crate::cli::{EnvironmentArgs, RunnerMode};
use crate::images::{DIND_TAG, HOST_BINARY, RUNNER_TAG};
use crate::io::env::{DockerCli, Environment, program};
use crate::io::pair::write_private;
use crate::pairs::{MARKER, PlanInput, listening, plan, slots};
use crate::side::Side;

const LISTENING_TIMEOUT: Duration = Duration::from_secs(300);
pub(super) const LOG: &str = "velnor-host.log";
pub(super) const PID: &str = "pid";
pub(super) const PLAN: &str = "plan.json";

#[derive(Debug, Clone)]
pub struct Pairs {
    pub host: PathBuf,
    pub trusted: u16,
    pub untrusted: u16,
    pub policy: Option<PathBuf>,
    pub labels: Vec<String>,
}

pub(super) fn runner_dir(dir: &Path) -> PathBuf {
    dir.join("runner")
}

async fn image(tag: &str) -> io::Result<String> {
    let id = program(
        "docker",
        &["image", "inspect", "--format", "{{.Id}}", tag],
        None,
    )
    .await
    .map_err(|_| io::Error::other(format!("{tag} is missing; fix: velnor-local build")))?;
    Ok(String::from_utf8_lossy(&id).trim().to_owned())
}

pub async fn default_host(exe: &Path) -> io::Result<PathBuf> {
    let owner = fs::metadata(exe).await?.uid();
    for root in exe.ancestors().skip(1) {
        let candidate = root.join(HOST_BINARY);
        let meta = match fs::symlink_metadata(&candidate).await {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let mode = meta.mode();
        if !meta.is_file() || meta.uid() != owner || mode & 0o022 != 0 || mode & 0o100 == 0 {
            return Err(io::Error::other(format!(
                "{} is not a regular, executable file owned by the owner of {} and not writable by others",
                candidate.display(),
                exe.display()
            )));
        }
        return Ok(candidate);
    }
    Err(io::Error::other(format!(
        "no {HOST_BINARY} in any folder above {}",
        exe.display()
    )))
}

pub(crate) async fn pairs(args: &EnvironmentArgs) -> io::Result<Option<Pairs>> {
    if args.runner == RunnerMode::Plain {
        if args.policy.is_some() {
            return Err(io::Error::other(
                "--policy needs --runner pair: plain containers have no job-start check",
            ));
        }
        return Ok(None);
    }
    let missing = |error: io::Error| {
        io::Error::other(format!(
            "velnor-host: {error}; fix: velnor-local build, --velnor-host <path>, or --runner plain"
        ))
    };
    let host = match &args.velnor_host {
        Some(explicit) => std::path::absolute(explicit),
        None => default_host(&std::env::current_exe()?).await,
    }
    .map_err(missing)?;
    fs::metadata(&host).await.map_err(missing)?;
    let policy = match &args.policy {
        Some(file) => Some(std::path::absolute(file)?),
        None => None,
    };
    for tag in [RUNNER_TAG, DIND_TAG] {
        image(tag).await?;
    }
    Ok(Some(Pairs {
        host,
        trusted: args.runners,
        untrusted: args.fork_runners,
        policy,
        labels: args.label.clone(),
    }))
}

impl Pairs {
    #[must_use]
    pub(crate) fn describe(&self) -> String {
        format!(
            "runner mode pair: {} trusted and {} untrusted pairs, {}",
            self.trusted,
            self.untrusted,
            self.host.display()
        )
    }

    pub(super) fn arguments(&self, repo: &Path) -> Vec<String> {
        let mut arguments = vec![
            "--velnor-host".to_owned(),
            self.host.display().to_string(),
            "--pairs".to_owned(),
            self.trusted.to_string(),
            "--fork-pairs".to_owned(),
            self.untrusted.to_string(),
        ];
        if self.policy.is_some() {
            arguments.extend(["--policy".to_owned(), "--repo".to_owned()]);
            arguments.push(repo.display().to_string());
        }
        for label in &self.labels {
            arguments.extend(["--label".to_owned(), label.clone()]);
        }
        arguments
    }

    pub(super) async fn wait(&self, dir: &Path, supervise: &mut Child) -> io::Result<Vec<String>> {
        let slots = slots(self.trusted, self.untrusted);
        let log = runner_dir(dir).join(LOG);
        let deadline = tokio::time::Instant::now() + LISTENING_TIMEOUT;
        loop {
            let text = fs::read_to_string(&log).await.unwrap_or_default();
            if listening(&text, &slots).len() == slots.len() {
                return Ok(slots);
            }
            if let Some(status) = supervise.try_wait()? {
                return Err(io::Error::other(format!(
                    "the authority and velnor-host exited with {status}; see runner/{LOG} and authority/supervise.log"
                )));
            }
            if tokio::time::Instant::now() > deadline {
                return Err(io::Error::other(format!(
                    "the pairs did not listen within {} s; see runner/{LOG}",
                    LISTENING_TIMEOUT.as_secs()
                )));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

pub(super) const POLICY: &str = "policy.json";
const MAX_POLICY_BYTES: u64 = 256 * 1024;

pub(super) async fn read_policy(file: &Path) -> io::Result<String> {
    let meta = fs::symlink_metadata(file).await?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_POLICY_BYTES {
        return Err(io::Error::other(format!(
            "--policy {} is not a regular file of 1 byte to 256 KiB",
            file.display()
        )));
    }
    fs::read_to_string(file).await
}

pub(super) async fn store_policy(state: &Path, text: &str) -> io::Result<()> {
    write_private(&state.join(POLICY), text).await
}

pub async fn write_plan(state: &Path, planned: &str) -> io::Result<()> {
    write_private(&state.join(PLAN), planned).await
}

pub(super) async fn spawn(
    env: &Environment<DockerCli>,
    host: &Path,
    authority: &str,
    token: &str,
    (trusted, untrusted): (u16, u16),
    policy: bool,
) -> io::Result<Child> {
    let state = runner_dir(&env.dir);
    fs::create_dir_all(&state).await?;
    fs::write(state.join(MARKER), "velnor-local\n").await?;
    let docker = program(
        "docker",
        &[
            "context",
            "inspect",
            "--format",
            "{{.Endpoints.docker.Host}}",
        ],
        None,
    )
    .await?;
    let (runner_image, dind_image) = (image(RUNNER_TAG).await?, image(DIND_TAG).await?);
    let sides = [
        (Side::Trusted, env.network(Side::Trusted), trusted),
        (Side::Untrusted, env.network(Side::Untrusted), untrusted),
    ];
    let planned = plan(&PlanInput {
        environment: &env.name,
        authority,
        token,
        docker: String::from_utf8_lossy(&docker).trim(),
        runner_image: &runner_image,
        dind_image: &dind_image,
        sides: &sides,
        policy: policy.then_some(POLICY),
    })
    .to_string();
    write_plan(&state, &planned).await?;
    let log = std::fs::File::create(state.join(LOG))?;
    let mut child = Command::new(host)
        .arg("--state")
        .arg(&state)
        .arg("local")
        .stdin(Stdio::piped())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?;
    if let Some(pid) = child.id() {
        write_private(&state.join(PID), &pid.to_string()).await?;
    }
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("velnor-host has no stdin"))?;
    stdin.write_all(planned.as_bytes()).await?;
    drop(stdin);
    Ok(child)
}
