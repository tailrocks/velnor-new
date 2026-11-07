// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Result;
use std::{
    io::Read,
    os::unix::fs::MetadataExt,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use velnor_actions_contract::{QUALIFICATION_DAEMON_IMAGE, QUALIFICATION_DAEMON_SCRIPT};
use velnor_actions_mise::{BoundedCommandStatus, BoundedCommandTermination, run_command_bounded};
const HOST: &str = "unix:///var/run/docker.sock";

pub(super) fn docker_pin(lock: &toml::Value) -> Result<(&str, &str, &str)> {
    let version = text(lock, "docker_version")?;
    let sha = text(lock, "docker_sha256")?;
    let url = text(lock, "docker_archive_url")?;
    if !velnor_actions_contract::config::is_exact_semver(version)
        || url
            != format!(
                "https://download.docker.com/linux/static/stable/x86_64/docker-{version}.tgz"
            )
        || !valid_id(sha)
    {
        return Err("invalid docker lock".into());
    }
    Ok((version, sha, url))
}

fn text<'a>(value: &'a toml::Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "missing docker lock field".into())
}

pub(super) fn valid_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn path(path: &Path) -> Result<String> {
    path.to_str()
        .filter(|s| !s.contains([',', '\n', '\r', '\0']))
        .map(str::to_owned)
        .ok_or_else(|| "invalid daemon path".into())
}

pub(super) fn read_container_id(path: &Path) -> Result<String> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )?;
    let mut file = std::fs::File::from(fd);
    let stat = file.metadata()?;
    if !stat.is_file()
        || stat.uid() != rustix::process::geteuid().as_raw()
        || stat.nlink() != 1
        || stat.len() > 65
    {
        return Err("invalid daemon container identity file".into());
    }
    let mut value = String::new();
    file.read_to_string(&mut value)?;
    let value = value.trim().to_owned();
    if !valid_id(&value) {
        return Err("invalid daemon container identity".into());
    }
    Ok(value)
}

pub(super) fn wait_child(child: &mut Child, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(child.try_wait()?.is_some())
}

pub(super) fn run(command: &mut Command, timeout: Duration) -> Result<String> {
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent");
    let output = run_command_bounded(command, 128 * 1024, timeout, Duration::from_secs(5));
    if output.status != BoundedCommandStatus::Exited(0)
        || output.termination != BoundedCommandTermination::Completed
    {
        return Err(format!(
            "daemon command failed: {:?} {:?}",
            output.status, output.termination
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub(super) fn download(url: &str, archive: &Path, timeout: Duration) -> Result<()> {
    let mut curl = Command::new("/usr/bin/curl");
    curl.args([
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-time",
        "120",
        "--max-filesize",
        "192000000",
        "--output",
    ])
    .arg(archive)
    .arg(url);
    run(&mut curl, timeout)?;
    Ok(())
}

pub(super) fn host_docker() -> Command {
    let mut command = Command::new("/usr/bin/docker");
    command.args(["--host", HOST]);
    command
}

pub(super) fn spawn_daemon(
    archive: &Path,
    temp: &Path,
    id_path: &Path,
    name: &str,
    sha: &str,
) -> Result<Child> {
    let archive_mount = format!("type=bind,src={},dst=/docker.tgz,readonly", path(archive)?);
    let temp_mount = format!("type=bind,src={},dst={}", path(temp)?, path(temp)?);
    let gid = rustix::process::getegid().as_raw().to_string();
    let mut command = host_docker();
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent")
        .args([
            "run",
            "--rm",
            "-i",
            "--privileged",
            "--network",
            "none",
            "--platform",
            "linux/amd64",
            "--name",
            name,
            "--cidfile",
        ])
        .arg(id_path)
        .args([
            "--mount",
            &archive_mount,
            "--mount",
            &temp_mount,
            "--entrypoint",
            "/bin/sh",
            QUALIFICATION_DAEMON_IMAGE,
            "-c",
            QUALIFICATION_DAEMON_SCRIPT,
            "velnor-q",
            sha,
            &gid,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    Ok(command.spawn()?)
}

pub(super) fn verify_container_fields(
    item: &serde_json::Value,
    id: &str,
    archive: &Path,
    temp: &Path,
    sha: &str,
) -> Result<()> {
    let gid = rustix::process::getegid().as_raw().to_string();
    let expected = serde_json::json!(["-c", QUALIFICATION_DAEMON_SCRIPT, "velnor-q", sha, gid]);
    if item["Id"].as_str() != Some(id)
        || item["Config"]["Image"].as_str() != Some(QUALIFICATION_DAEMON_IMAGE)
        || item["Config"]["Entrypoint"] != serde_json::json!(["/bin/sh"])
        || item["Config"]["Cmd"] != expected
        || item["Config"]["OpenStdin"].as_bool() != Some(true)
        || item["HostConfig"]["Privileged"].as_bool() != Some(true)
        || item["HostConfig"]["NetworkMode"].as_str() != Some("none")
    {
        return Err("daemon container identity mismatch".into());
    }
    let mounts = item["Mounts"]
        .as_array()
        .filter(|values| values.len() == 2)
        .ok_or("daemon mounts")?;
    for (source, destination, writable) in [
        (path(archive)?, "/docker.tgz".to_owned(), false),
        (path(temp)?, path(temp)?, true),
    ] {
        if mounts
            .iter()
            .filter(|mount| {
                mount["Type"].as_str() == Some("bind")
                    && mount["Source"].as_str() == Some(&source)
                    && mount["Destination"].as_str() == Some(destination.as_str())
                    && mount["RW"].as_bool() == Some(writable)
            })
            .count()
            != 1
        {
            return Err("daemon mount identity mismatch".into());
        }
    }
    Ok(())
}

pub(crate) fn private_docker(container_id: &str) -> Result<Command> {
    if !valid_id(container_id) {
        return Err("invalid daemon container identity".into());
    }
    let mut command = host_docker();
    command.args([
        "exec",
        container_id,
        "/native/docker",
        "--host",
        "unix:///run/velnor-q/docker.sock",
    ]);
    Ok(command)
}
