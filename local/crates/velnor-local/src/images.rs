// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

pub const RUNNER_TAG: &str = "velnor-local/pair-runner";
pub const DIND_TAG: &str = "velnor-local/pair-dind";
pub const PROBE_TAG: &str = "velnor-local/pair-probe";
pub const TARGETS: [&str; 3] = [RUNNER_TAG, DIND_TAG, PROBE_TAG];
const RUST_IMAGE: &str =
    "rust:1.99.1-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07";
const TARGET_VOLUME: &str = "velnor-arm64-target";
const CARGO_VOLUME: &str = "velnor-arm64-cargo";
const CARGO_HOME: &str = "/cargo-home";
const HELPERS: &str = r##"rustup target add aarch64-unknown-linux-musl \
    && cargo build --locked --release --manifest-path /src/crates/velnor-runner/Cargo.toml --target aarch64-unknown-linux-musl \
      -p velnor-resource-probe --bin velnor-resource-probe --bin velnor-storage-init --bin velnor-job-start-hook \
    && mkdir -p /out/resource-probe /out/storage-init /out/job-start-hook /out/job-started-hook \
    && cp /target/aarch64-unknown-linux-musl/release/velnor-resource-probe /out/resource-probe/resource-probe \
    && cp /target/aarch64-unknown-linux-musl/release/velnor-storage-init /out/storage-init/storage-init \
    && cp /target/aarch64-unknown-linux-musl/release/velnor-job-start-hook /out/job-start-hook/job-start-hook \
    && printf "#!/bin/sh\nset -eu\nexec /velnor/job-start-hook \"\$@\"\n" > /out/job-started-hook/velnor-job-start-hook.sh \
    && chmod 0555 /out/*/* \
    && chown -R "$(stat -c %u:%g /out)" /out"##;

pub const HOST_BUILD: [&str; 10] = [
    "build",
    "--release",
    "--locked",
    "--manifest-path",
    "crates/velnor-runner/Cargo.toml",
    "-p",
    "velnor-runner-cli",
    "--bin",
    "velnor-host",
    "--features=local-mode",
];
pub const HOST_BINARY: &str = "crates/velnor-runner/target/release/velnor-host";
pub const RELEASE_IDENTITY: [&str; 2] = [
    "VELNOR_RUNNER_BUILD_SOURCE_SHA",
    "VELNOR_RUNNER_BUILD_HELPER_SHA256",
];

#[derive(Debug)]
pub struct Lock(BTreeMap<String, String>);

impl Lock {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut strings = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let number = index + 1;
            let malformed =
                || format!(".velnor/runner.lock line {number} is not a `key = value` assignment");
            let (key, value) = line.split_once(" = ").ok_or_else(malformed)?;
            let key_ok = !key.is_empty()
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
            let text = value
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'));
            let list = value.starts_with('[') && value.ends_with(']');
            if !key_ok || (text.is_none() && !list && value.parse::<i64>().is_err()) {
                return Err(malformed());
            }
            if !seen.insert(key.to_owned()) {
                return Err(format!(
                    ".velnor/runner.lock line {number} assigns {key} a second time"
                ));
            }
            if let Some(text) = text {
                strings.insert(key.to_owned(), text.to_owned());
            }
        }
        Ok(Self(strings))
    }

    pub fn get(&self, key: &str) -> Result<&str, String> {
        self.0
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!(".velnor/runner.lock has no {key}"))
    }
}

fn flags(flag: &str, values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .flat_map(|value| [flag.to_owned(), value])
        .collect()
}

fn checksum(lock: &Lock, name: &str, key: &str) -> Result<String, String> {
    Ok(format!("{name}={}", lock.get(key)?))
}

fn build(tag: &str, mut args: Vec<String>, path: String) -> Vec<String> {
    let mut command = ["buildx", "build", "--platform", "linux/arm64"]
        .map(str::to_owned)
        .to_vec();
    command.append(&mut args);
    command.extend([
        "--tag".to_owned(),
        tag.to_owned(),
        "--load".to_owned(),
        path,
    ]);
    command
}

fn target_volume(root: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, root.as_bytes());
    let suffix = digest.as_ref()[..8]
        .iter()
        .fold(String::new(), |mut hex, byte| {
            write!(hex, "{byte:02x}").ok();
            hex
        });
    format!("{TARGET_VOLUME}-{suffix}")
}

fn helpers(root: &str, stages: &str, source_sha: &str) -> Vec<String> {
    [
        "run",
        "--rm",
        "--platform",
        "linux/arm64",
        "-v",
        &format!("{root}:/src:ro"),
        "-v",
        &format!("{}:/target", target_volume(root)),
        "-v",
        &format!("{stages}:/out"),
        "-v",
        &format!("{CARGO_VOLUME}:{CARGO_HOME}"),
        "-v",
        &format!("{}:/usr/local/rustup", rustup_volume()),
        "-e",
        "CARGO_TARGET_DIR=/target",
        "-e",
        &format!("CARGO_HOME={CARGO_HOME}"),
        "-e",
        &format!("VELNOR_SOURCE_SHA={source_sha}"),
        RUST_IMAGE,
        "sh",
        "-c",
        HELPERS,
    ]
    .map(str::to_owned)
    .to_vec()
}

fn rustup_volume() -> String {
    let digest = RUST_IMAGE
        .rsplit_once("sha256:")
        .map_or("", |(_, digest)| digest);
    format!("velnor-arm64-rustup-{}", &digest[..digest.len().min(12)])
}

pub fn refuse_dirty(porcelain: &str) -> Result<(), String> {
    let changed: Vec<&str> = porcelain.lines().collect();
    if changed.is_empty() {
        return Ok(());
    }
    Err(format!(
        "the images are built from the working tree but named for HEAD; commit or stash these first: {}",
        changed.join("; ")
    ))
}

pub fn plan(
    lock: &Lock,
    root: &Path,
    stages: &Path,
    source_sha: &str,
) -> Result<Vec<Vec<String>>, String> {
    let (root, stages) = (root.display().to_string(), stages.display().to_string());
    let base = format!(
        "{}=docker-image://{}@{}",
        lock.get("base_image")?,
        lock.get("base_image_name")?,
        lock.get("base_image_arm64_digest")?,
    );
    let context = |name: &str, stage: &str| format!("{name}={stages}/{stage}");
    let helpers = helpers(&root, &stages, source_sha);
    let sha = format!("VELNOR_SOURCE_SHA={source_sha}");
    let mut runner = flags(
        "--build-arg",
        vec![
            sha.clone(),
            checksum(lock, "RUNNER_ARM64_SHA256", "runner_arm64_sha256")?,
            checksum(lock, "DOCKER_ARM64_SHA256", "docker_arm64_sha256")?,
            checksum(lock, "COMPOSE_ARM64_SHA256", "compose_arm64_sha256")?,
            checksum(lock, "BUILDX_ARM64_SHA256", "buildx_arm64_sha256")?,
            checksum(lock, "NODE_ARM64_SHA256", "node_arm64_sha256")?,
        ],
    );
    runner.extend(flags(
        "--build-context",
        vec![
            base.clone(),
            context("resource_probe", "resource-probe"),
            context("storage_init", "storage-init"),
            context("job_start_hook", "job-start-hook"),
            context("job_started_hook", "job-started-hook"),
        ],
    ));
    let mut dind = flags(
        "--build-arg",
        vec![
            sha,
            checksum(lock, "DOCKER_ARM64_SHA256", "docker_arm64_sha256")?,
        ],
    );
    dind.extend(flags(
        "--build-context",
        vec![base, context("resource_probe", "resource-probe")],
    ));
    let mut probe = flags("--build-arg", vec![format!("SOURCE_SHA={source_sha}")]);
    probe.extend(flags(
        "--build-context",
        vec![context("resource_probe", "resource-probe")],
    ));
    Ok(vec![
        helpers,
        build(
            RUNNER_TAG,
            runner,
            format!("{root}/images/runner/ubuntu-26.04"),
        ),
        build(DIND_TAG, dind, format!("{root}/images/dind")),
        build(PROBE_TAG, probe, format!("{root}/images/resource-probe")),
    ])
}
