// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use clap::Parser;
use velnor_local::cli::{Cli, Command};
use velnor_local::doctor::{Part, healthy, render};
use velnor_local::images::TARGETS;
use velnor_local::io::doctor::{inspect, require_local};
use velnor_local::io::env::Engine;
use velnor_local::pairs::Limits;

use crate::support::temp_dir;

const GIB: u64 = 1 << 30;

struct Docker {
    info: Option<&'static str>,
    memory: Option<&'static str>,
    context: bool,
    endpoint: &'static str,
    images: BTreeSet<&'static str>,
}

impl Engine for Docker {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        let answer = match args {
            ["info", "--format", "{{.MemTotal}}"] => self.memory.map(str::to_owned),
            ["info", ..] => self.info.map(str::to_owned),
            ["context", "inspect", ..] => self.context.then(|| self.endpoint.to_owned()),
            ["image", "inspect", .., tag] => {
                self.images.contains(tag).then(|| "sha256:aa".to_owned())
            }
            _ => None,
        };
        std::future::ready(answer.ok_or_else(|| io::Error::other("no such thing")))
    }

    fn logged(&self, _: &[&str], _: &Path) -> impl Future<Output = io::Result<bool>> + Send {
        std::future::ready(Ok(true))
    }
}

const ALL: [&str; 6] = [
    "velnor-local/runner-server",
    "velnor-local/runner",
    "velnor-local/cache-server",
    TARGETS[0],
    TARGETS[1],
    TARGETS[2],
];

fn docker(without: &[&str]) -> Docker {
    Docker {
        info: Some("OrbStack|aarch64"),
        memory: Some("17179869184"),
        context: true,
        endpoint: "unix:///Users/me/.orbstack/run/docker.sock",
        images: ALL
            .into_iter()
            .filter(|tag| !without.contains(tag))
            .collect(),
    }
}

fn host(name: &str, local_mode: bool) -> PathBuf {
    let path = temp_dir(name).join("velnor-host");
    let status = if local_mode { 0 } else { 2 };
    std::fs::write(&path, format!("#!/bin/sh\nexit {status}\n"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    path
}

async fn report(docker: &Docker, host: &Path) -> (Vec<Part>, String) {
    report_with(docker, host, Limits::default().runner_memory_bytes).await
}

async fn report_with(docker: &Docker, host: &Path, runner_memory: u64) -> (Vec<Part>, String) {
    let parts = inspect(docker, Some(host), runner_memory).await;
    let text = render(&parts);
    (parts, text)
}

#[tokio::test]
async fn everything_present_prints_ok_for_every_part_and_is_healthy() {
    let (parts, text) = report(&docker(&[]), &host("doctor-ok", true)).await;
    assert!(healthy(&parts), "{text}");
    assert_eq!(text.lines().count(), 5, "{text}");
    assert!(text.lines().all(|line| line.starts_with("ok ")), "{text}");
    assert!(
        text.contains("docker: OrbStack, linux/arm64, unix:///Users/me/.orbstack/run/docker.sock"),
        "{text}"
    );
}

#[tokio::test]
async fn missing_pair_images_print_miss_with_the_build_fix_and_are_unhealthy() {
    let (parts, text) = report(&docker(&TARGETS), &host("doctor-pairs", true)).await;
    assert!(!healthy(&parts), "{text}");
    let miss: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("miss"))
        .collect();
    assert_eq!(miss.len(), 1, "{text}");
    assert!(miss[0].contains("pair images"), "{text}");
    assert!(miss[0].ends_with("fix: velnor-local build"), "{text}");
}

#[tokio::test]
async fn a_missing_stand_in_image_names_its_docker_build() {
    let (parts, text) = report(
        &docker(&["velnor-local/runner-server"]),
        &host("doctor-standin", true),
    )
    .await;
    assert!(!healthy(&parts), "{text}");
    assert!(
        text.contains("fix: docker build -t velnor-local/runner-server local/docker/runner-server"),
        "{text}"
    );
    assert!(
        !text.contains("docker build -t velnor-local/runner local"),
        "{text}"
    );
}

#[tokio::test]
async fn a_host_without_local_mode_or_without_a_file_is_a_miss_with_the_build_fix() {
    let plain = host("doctor-release", false);
    let (parts, text) = report(&docker(&[]), &plain).await;
    assert!(!healthy(&parts), "{text}");
    assert!(text.contains("miss  velnor-host with local mode"), "{text}");
    assert!(
        text.contains("fix: build or install velnor-host at"),
        "{text}"
    );
}

#[tokio::test]
async fn unreachable_docker_and_other_architectures_are_misses() {
    let down = Docker {
        info: None,
        memory: None,
        context: true,
        endpoint: "unix:///var/run/docker.sock",
        images: BTreeSet::new(),
    };
    let (parts, text) = report(&down, &host("doctor-down", true)).await;
    assert!(!healthy(&parts), "{text}");
    assert!(text.contains("miss  docker"), "{text}");
    assert!(
        text.contains("fix: start OrbStack or Docker Desktop"),
        "{text}"
    );
    let fixes = text.matches("fix:").count();
    assert_eq!(fixes, 1, "{text}");
    assert!(!text.contains("docker build"), "{text}");
    assert!(
        text.contains("n/a   pair images: ") && text.contains("unknown (Docker unreachable)"),
        "{text}"
    );
    assert!(!text.contains("missing"), "{text}");
    let amd = Docker {
        info: Some("Docker Desktop|x86_64"),
        ..docker(&[])
    };
    let (parts, text) = report(&amd, &host("doctor-amd", true)).await;
    assert!(!healthy(&parts), "{text}");
    assert!(text.contains("miss  docker: Docker, linux/amd64"), "{text}");
}

#[test]
fn doctor_takes_an_optional_velnor_host_and_status_needs_a_dir() {
    let doctor = Cli::try_parse_from(["velnor-local", "doctor", "--velnor-host", "/x/velnor-host"]);
    assert!(matches!(
        doctor.map(|cli| cli.command),
        Ok(Command::Doctor(_))
    ));
    assert!(Cli::try_parse_from(["velnor-local", "status"]).is_err());
    let status = Cli::try_parse_from(["velnor-local", "status", "--dir", "/s"]);
    assert!(matches!(
        status.map(|cli| cli.command),
        Ok(Command::Status(_))
    ));
}

#[test]
fn the_doctor_command_exits_1_and_prints_a_fix_when_a_part_is_missing() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-local"))
        .args(["doctor", "--velnor-host", "/nonexistent/velnor-host"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("miss  velnor-host with local mode"), "{text}");
    assert!(
        text.contains("fix: build or install velnor-host at"),
        "{text}"
    );
}

#[tokio::test]
async fn an_unreadable_docker_context_is_a_miss_with_the_docker_fix() {
    let blind = Docker {
        context: false,
        ..docker(&[])
    };
    let (parts, text) = report(&blind, &host("doctor-context", true)).await;
    assert!(!healthy(&parts), "{text}");
    assert!(
        text.contains("miss  docker: context not readable"),
        "{text}"
    );
    assert!(
        text.contains("fix: start OrbStack or Docker Desktop"),
        "{text}"
    );
}

#[tokio::test]
async fn an_explicit_missing_host_names_that_path_in_the_fix() {
    let (parts, text) = report(&docker(&[]), Path::new("/nonexistent/velnor-host")).await;
    assert!(!healthy(&parts), "{text}");
    assert!(
        text.contains(
            "fix: build or install velnor-host at /nonexistent/velnor-host, or omit --velnor-host"
        ),
        "{text}"
    );
    assert!(!text.contains("fix: velnor-local build"), "{text}");
}

#[tokio::test]
async fn a_vm_of_8_gib_cannot_hold_a_default_pair_and_the_fix_names_orbstack() {
    let small = Docker {
        memory: Some("8589934592"),
        ..docker(&[])
    };
    let (parts, text) = report(&small, &host("doctor-vm-small", true)).await;
    assert!(!healthy(&parts), "{text}");
    let miss: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("miss"))
        .collect();
    assert_eq!(miss.len(), 1, "{text}");
    assert!(miss[0].contains("docker vm memory"), "{text}");
    assert!(miss[0].contains("8 GiB"), "{text}");
    assert!(miss[0].contains("9 GiB"), "{text}");
    assert!(
        miss[0].contains("OrbStack Settings > System > Memory limit"),
        "{text}"
    );
    assert!(miss[0].contains("--runner-memory"), "{text}");
}

#[tokio::test]
async fn a_vm_of_16_gib_is_ok_and_a_smaller_runner_memory_flips_the_8_gib_case() {
    let (parts, text) = report(&docker(&[]), &host("doctor-vm-big", true)).await;
    assert!(healthy(&parts), "{text}");
    assert!(text.contains("ok    docker vm memory"), "{text}");
    let small = Docker {
        memory: Some("8589934592"),
        ..docker(&[])
    };
    let (parts, text) = report_with(&small, &host("doctor-vm-flag", true), 2 * GIB).await;
    assert!(healthy(&parts), "{text}");
    assert!(text.contains("ok    docker vm memory"), "{text}");
}

#[tokio::test]
async fn an_unreadable_vm_memory_is_unknown_and_not_a_miss() {
    for memory in [None, Some("lots")] {
        let blind = Docker {
            memory,
            ..docker(&[])
        };
        let (parts, text) = report(&blind, &host("doctor-vm-blind", true)).await;
        assert!(healthy(&parts), "{text}");
        assert!(text.contains("n/a   docker vm memory"), "{text}");
        assert!(!text.contains("miss"), "{text}");
    }
}

#[test]
fn doctor_takes_a_runner_memory_that_must_be_a_valid_size() {
    let parsed = Cli::try_parse_from(["velnor-local", "doctor", "--runner-memory", "2g"]);
    assert!(matches!(
        parsed.map(|cli| cli.command),
        Ok(Command::Doctor(args)) if args.runner_memory == 2 * GIB
    ));
    assert!(Cli::try_parse_from(["velnor-local", "doctor", "--runner-memory", "0"]).is_err());
}

const REMOTE_FIX: &str = "fix: velnor-local needs a Docker daemon on this machine; to use a remote daemon or dind, run velnor-local and velnor-host inside it (Linux)";

#[tokio::test]
async fn a_remote_docker_endpoint_is_a_miss_with_the_run_it_inside_fix() {
    let remote = Docker {
        endpoint: "tcp://10.0.0.5:2375",
        ..docker(&[])
    };
    let (parts, text) = report(&remote, &host("doctor-remote", true)).await;
    assert!(!healthy(&parts), "{text}");
    assert!(
        text.contains("miss  docker: tcp://10.0.0.5:2375 is not a local socket"),
        "{text}"
    );
    assert!(text.contains(REMOTE_FIX), "{text}");
    assert_eq!(text.matches("fix:").count(), 1, "{text}");
    let unreachable = Docker {
        info: None,
        ..remote
    };
    let (_, text) = report(&unreachable, &host("doctor-remote-down", true)).await;
    assert!(text.contains("not a local socket"), "{text}");
    assert!(!text.contains("not reachable"), "{text}");
}

#[tokio::test]
async fn local_unix_sockets_stay_ok() {
    for endpoint in [
        "unix:///Users/x/.orbstack/run/docker.sock",
        "unix:///var/run/docker.sock",
    ] {
        let local = Docker {
            endpoint,
            ..docker(&[])
        };
        let (parts, text) = report(&local, &host("doctor-unix", true)).await;
        assert!(healthy(&parts), "{endpoint}: {text}");
        assert!(text.contains(&format!("linux/arm64, {endpoint}")), "{text}");
        assert!(require_local(&local).await.is_ok(), "{endpoint}");
    }
}

#[tokio::test]
async fn build_up_and_run_stop_on_a_remote_endpoint_before_any_work() {
    let remote = Docker {
        endpoint: "ssh://me@box",
        ..docker(&[])
    };
    let refused = require_local(&remote).await.map_err(|e| e.to_string());
    assert!(
        refused.is_err_and(|message| message.contains("ssh://me@box")
            && message.contains("needs a Docker daemon on this machine")),
    );
    let blind = Docker {
        context: false,
        ..docker(&[])
    };
    assert!(
        require_local(&blind).await.is_ok(),
        "an unreadable context is the later steps' error"
    );
}

#[test]
fn the_cli_refuses_a_remote_daemon_in_build_up_and_run() {
    if std::process::Command::new("docker")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let dir = temp_dir("remote-cli");
    let dir = dir.to_string_lossy().into_owned();
    for args in [
        vec!["build", "--root", &dir],
        vec!["up", "--dir", &dir, "--repo", &dir],
        vec!["run", "hello", "--dir", &dir, "--repo", &dir],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-local"))
            .args(&args)
            .env("DOCKER_HOST", "tcp://10.0.0.5:2375")
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        let text = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {text}");
        assert!(
            text.contains("tcp://10.0.0.5:2375 is not a local socket"),
            "{args:?}: {text}"
        );
    }
}
