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
use velnor_local::io::doctor::inspect;
use velnor_local::io::env::Engine;

use crate::support::temp_dir;

struct Docker {
    info: Option<&'static str>,
    images: BTreeSet<&'static str>,
}

impl Engine for Docker {
    fn docker(&self, args: &[&str]) -> impl Future<Output = io::Result<String>> + Send {
        let answer = match args {
            ["info", ..] => self.info.map(str::to_owned),
            ["context", "inspect", ..] => {
                Some("unix:///Users/me/.orbstack/run/docker.sock".to_owned())
            }
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
    let parts = inspect(docker, Some(host)).await;
    let text = render(&parts);
    (parts, text)
}

#[tokio::test]
async fn everything_present_prints_ok_for_every_part_and_is_healthy() {
    let (parts, text) = report(&docker(&[]), &host("doctor-ok", true)).await;
    assert!(healthy(&parts), "{text}");
    assert_eq!(text.lines().count(), 4, "{text}");
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
    assert!(text.contains("fix: velnor-local build"), "{text}");
    let (parts, text) = report(&docker(&[]), Path::new("/nonexistent/velnor-host")).await;
    assert!(!healthy(&parts), "{text}");
    assert!(text.contains("fix: velnor-local build"), "{text}");
}

#[tokio::test]
async fn unreachable_docker_and_other_architectures_are_misses() {
    let down = Docker {
        info: None,
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
    assert!(text.contains("fix: velnor-local build"), "{text}");
}
