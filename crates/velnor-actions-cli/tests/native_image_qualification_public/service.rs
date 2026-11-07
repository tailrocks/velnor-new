// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Child,
    time::{Duration, Instant},
};
use velnor_actions_contract::{QUALIFICATION_DAEMON_IMAGE, QualificationDaemonDescriptor};
#[path = "service/commands.rs"]
mod commands;
pub(super) use commands::private_docker;
use commands::{
    docker_pin, download, host_docker, path, read_container_id, run, spawn_daemon, valid_id,
    verify_container_fields, wait_child,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const START_TIMEOUT: Duration = Duration::from_secs(150);
pub(super) const START_RESERVE: Duration = Duration::from_secs(130 + 150 + 15);
const STOP_TIMEOUT: Duration = Duration::from_secs(20);
const DOCKER_COMMAND_BUDGET_SECS: u64 = 15;
const INSPECTED_REMOVAL_BUDGET_SECS: u64 = 4 * DOCKER_COMMAND_BUDGET_SECS;
pub(super) const CLEANUP_RESERVE: Duration = Duration::from_secs(
    20 + 2 * INSPECTED_REMOVAL_BUDGET_SECS + 10 + 5 + DOCKER_COMMAND_BUDGET_SECS,
);

pub(super) struct Lease {
    child: Child,
    deadline: Instant,
    temp: PathBuf,
    name: String,
    archive: PathBuf,
    id_path: PathBuf,
    archive_sha: String,
    container_id: Option<String>,
    daemon_id: Option<String>,
    instance: Option<(String, u64)>,
    closed: bool,
}

impl Lease {
    pub(super) fn start(root: &Path, temp: &Path, deadline: Instant) -> Result<Self> {
        let lock: toml::Value =
            toml::from_str(&fs::read_to_string(root.join(".velnor/runner.lock"))?)?;
        let (version, sha, url) = docker_pin(&lock)?;
        let private = temp.join("velnor-qualification-service");
        fs::DirBuilder::new().mode(0o700).create(&private)?;
        let archive = private.join("docker.tgz");
        let work_deadline = deadline
            .checked_sub(CLEANUP_RESERVE)
            .ok_or("qualification deadline")?;
        download(
            url,
            &archive,
            budget(work_deadline, Duration::from_secs(130))?,
        )?;
        if super::hash_file(&archive, deadline)? != sha {
            return Err("docker archive digest mismatch".into());
        }
        let mut nonce = [0_u8; 16];
        fs::File::open("/dev/urandom")?.read_exact(&mut nonce)?;
        let name = format!("velnor-q-{}", super::hex(&nonce));
        let id_path = private.join("container.id");
        let child = spawn_daemon(&archive, temp, &id_path, &name, sha)?;
        let mut lease = Self {
            child,
            deadline,
            temp: temp.to_path_buf(),
            name,
            archive,
            id_path,
            archive_sha: sha.to_owned(),
            container_id: None,
            daemon_id: None,
            instance: None,
            closed: false,
        };
        let (container_id, daemon_id) = lease.await_ready(version)?;
        lease.verify_container(&container_id, false)?;
        lease.container_id = Some(container_id.clone());
        lease.daemon_id = Some(daemon_id.clone());
        let (started_at, pid) = lease.instance(&container_id)?;
        lease.instance = Some((started_at.clone(), pid));
        lease.write_descriptor(temp, container_id, daemon_id, started_at, pid)?;
        Ok(lease)
    }

    pub(super) fn verify(&self) -> Result<()> {
        let id = self
            .container_id
            .as_deref()
            .ok_or("missing container identity")?;
        if Some(self.instance(id)?) != self.instance {
            return Err("qualification daemon instance changed".into());
        }
        let mut command = private_docker(id)?;
        command.args(["info", "--format", "{{.ID}}"]);
        let actual = run(&mut command, self.timeout(Duration::from_secs(10), false)?)?;
        if Some(actual.trim()) != self.daemon_id.as_deref() {
            return Err("qualification daemon identity mismatch".into());
        }
        Ok(())
    }

    pub(super) fn container_id(&self) -> Result<&str> {
        self.container_id
            .as_deref()
            .ok_or_else(|| "missing container identity".into())
    }

    pub(super) fn close(mut self) -> Result<()> {
        self.shutdown()
    }

    fn write_descriptor(
        &self,
        temp: &Path,
        container_id: String,
        daemon_id: String,
        container_started_at: String,
        process: u64,
    ) -> Result<()> {
        let descriptor = QualificationDaemonDescriptor {
            container_id,
            daemon_id,
            container_started_at,
            container_pid: process,
            archive_sha256: self.archive_sha.clone(),
            archive_path: path(&self.archive)?,
        };
        let bytes = serde_json::to_vec(&descriptor)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(temp.join("velnor-qualification-service.json"))?;
        file.write_all(&bytes)?;
        Ok(())
    }

    fn instance(&self, id: &str) -> Result<(String, u64)> {
        self.verify_container(id, false)?;
        let mut command = host_docker();
        command.args(["inspect", id]);
        let output = run(&mut command, self.timeout(Duration::from_secs(10), false)?)?;
        let value = velnor_actions_contract::parse_strict_json(&output)?;
        let Some([item]) = value.as_array().map(Vec::as_slice) else {
            return Err("invalid daemon instance".into());
        };
        let start = item["State"]["StartedAt"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("missing daemon start")?;
        let pid = item["State"]["Pid"]
            .as_u64()
            .filter(|pid| *pid > 0)
            .ok_or("missing daemon pid")?;
        Ok((start.to_owned(), pid))
    }

    fn work_deadline(&self) -> Result<Instant> {
        self.deadline
            .checked_sub(CLEANUP_RESERVE)
            .ok_or_else(|| "qualification deadline".into())
    }

    fn timeout(&self, maximum: Duration, cleanup: bool) -> Result<Duration> {
        budget(
            if cleanup {
                self.deadline
            } else {
                self.work_deadline()?
            },
            maximum,
        )
    }

    fn await_ready(&mut self, version: &str) -> Result<(String, String)> {
        let deadline = (Instant::now() + START_TIMEOUT).min(self.work_deadline()?);
        while Instant::now() < deadline {
            if self.child.try_wait()?.is_some() {
                return Err("qualification daemon exited during startup".into());
            }
            if self.id_path.exists() {
                let Ok(id) = read_container_id(&self.id_path) else {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                };
                self.verify_container(&id, false)?;
                let mut command = private_docker(&id)?;
                command.args(["version", "--format", "{{.Server.Version}}"]);
                if run(&mut command, self.timeout(Duration::from_secs(5), false)?)
                    .is_ok_and(|s| s.trim() == version)
                {
                    let mut info = private_docker(&id)?;
                    info.args(["info", "--format", "{{.ID}}"]);
                    let daemon_id = run(&mut info, self.timeout(Duration::from_secs(10), false)?)?;
                    if daemon_id.trim().is_empty() {
                        return Err("empty daemon identity".into());
                    }
                    return Ok((id, daemon_id.trim().to_owned()));
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err("qualification daemon startup timed out".into())
    }

    fn shutdown(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        drop(self.child.stdin.take());
        let timeout = self.timeout(STOP_TIMEOUT, true)?;
        if !wait_child(&mut self.child, timeout)? {
            self.force_owned_container()?;
            let timeout = self.timeout(Duration::from_secs(10), true)?;
            if !wait_child(&mut self.child, timeout)? {
                self.child.kill()?;
                let timeout = self.timeout(Duration::from_secs(5), true)?;
                if !wait_child(&mut self.child, timeout)? {
                    return Err("qualification docker client did not exit".into());
                }
            }
        }
        self.force_owned_container()?;
        self.ensure_container_absent()?;
        self.closed = true;
        Ok(())
    }

    fn force_owned_container(&self) -> Result<()> {
        let Some(id) = self.locate_container()? else {
            return Ok(());
        };
        if self
            .container_id
            .as_ref()
            .is_some_and(|expected| expected != &id)
        {
            return Err("qualification container identity changed".into());
        }
        if let Ok(expected) = read_container_id(&self.id_path)
            && id != expected
        {
            return Err("qualification container identity changed".into());
        }
        self.verify_container(&id, true)?;
        let mut command = host_docker();
        command.args(["rm", "--force", &id]);
        run(&mut command, self.timeout(Duration::from_secs(10), true)?)?;
        Ok(())
    }

    fn ensure_container_absent(&self) -> Result<()> {
        if self.locate_container()?.is_some() {
            return Err("qualification daemon container remains".into());
        }
        Ok(())
    }

    fn locate_container(&self) -> Result<Option<String>> {
        let name = &self.name;
        let filter = format!("name=^/{name}$");
        let mut command = host_docker();
        command.args([
            "ps",
            "--all",
            "--no-trunc",
            "--filter",
            &filter,
            "--format",
            "{{.ID}}",
        ]);
        let output = run(&mut command, self.timeout(Duration::from_secs(10), true)?)?;
        let ids = output
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        match ids.as_slice() {
            [] => Ok(None),
            [id] if valid_id(id) => Ok(Some((*id).to_owned())),
            _ => Err("ambiguous qualification daemon container".into()),
        }
    }

    fn verify_container(&self, id: &str, cleanup: bool) -> Result<()> {
        if !valid_id(id) {
            return Err("invalid daemon container identity".into());
        }
        let mut command = host_docker();
        command.args(["inspect", id]);
        let output = run(
            &mut command,
            self.timeout(Duration::from_secs(10), cleanup)?,
        )?;
        let rows = velnor_actions_contract::parse_strict_json(&output)?;
        let Some([item]) = rows.as_array().map(Vec::as_slice) else {
            return Err("invalid daemon container response".into());
        };
        verify_container_fields(item, id, &self.archive, &self.temp, &self.archive_sha)?;
        let mut image = host_docker();
        image.args(["image", "inspect", QUALIFICATION_DAEMON_IMAGE]);
        let output = run(&mut image, self.timeout(Duration::from_secs(10), cleanup)?)?;
        let value = velnor_actions_contract::parse_strict_json(&output)?;
        let Some([image]) = value.as_array().map(Vec::as_slice) else {
            return Err("invalid daemon image response".into());
        };
        if item["Image"] != image["Id"] {
            return Err("daemon image identity mismatch".into());
        }
        Ok(())
    }
}

fn budget(deadline: Instant, maximum: Duration) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .and_then(|remaining| remaining.checked_sub(Duration::from_secs(5)))
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| remaining.min(maximum))
        .ok_or_else(|| "qualification deadline".into())
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("qualification daemon cleanup failed: {error}");
        }
    }
}
