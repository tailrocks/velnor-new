// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

pub const RUNNER_TAG: &str = "velnor-local/pair-runner";
pub const DIND_TAG: &str = "velnor-local/pair-dind";
pub const PROBE_TAG: &str = "velnor-local/pair-probe";
pub const TARGETS: [&str; 3] = [RUNNER_TAG, DIND_TAG, PROBE_TAG];
const RUST_IMAGE: &str =
    "rust:1.99.1-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07";
const RUST_DIGEST: &str = "8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07";
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
    && chmod 0555 /out/*/*"##;

#[derive(Debug)]
pub struct Lock(BTreeMap<String, String>);

impl Lock {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        Self(
            text.lines()
                .filter_map(|line| {
                    let (key, value) = line.split_once(" = ")?;
                    let value = value.strip_prefix('"')?.strip_suffix('"')?;
                    Some((key.to_owned(), value.to_owned()))
                })
                .collect(),
        )
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
    let helpers = [
        "run",
        "--rm",
        "--platform",
        "linux/arm64",
        "-v",
        &format!("{root}:/src:ro"),
        "-v",
        &format!("{TARGET_VOLUME}:/target"),
        "-v",
        &format!("{stages}:/out"),
        "-v",
        &format!("{CARGO_VOLUME}:{CARGO_HOME}"),
        "-v",
        &format!(
            "velnor-arm64-rustup-{}:/usr/local/rustup",
            &RUST_DIGEST[..12]
        ),
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
    .to_vec();
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
