// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    RunnerPublicationDockerfileContext, native_image_candidate_inputs,
    runner_publication_dockerfile_context, runner_publication_source_inputs,
};

use crate::{OrchestratorError, internal::internal};

use super::{CandidateRecord, RunnerLockVersions, hex, image_name};

pub(super) fn verify_source_inputs(
    candidate: &CandidateRecord,
    source_files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), OrchestratorError> {
    if source_files.len() != source_paths(candidate.profile)?.len()
        || candidate
            .source_inputs
            .keys()
            .any(|key| !source_files.contains_key(key))
    {
        return Err(internal("image_publication_source_inventory"));
    }
    for (path, expected) in &candidate.source_inputs {
        let bytes = source_files
            .get(path)
            .ok_or_else(|| internal("image_publication_source_inventory"))?;
        if expected.as_str() != hex(&Sha256::digest(bytes)) {
            return Err(internal("image_publication_source_digest"));
        }
    }
    let lock_bytes = source_files
        .get(".velnor/runner.lock")
        .ok_or_else(|| internal("image_publication_lock_missing"))?;
    let lock: RunnerLockVersions = toml::from_str(
        std::str::from_utf8(lock_bytes).map_err(|_| internal("image_publication_lock_utf8"))?,
    )
    .map_err(|_| internal("image_publication_lock_parse"))?;
    if [
        &lock.runner,
        &lock.docker,
        &lock.buildx,
        &lock.compose,
        &lock.node,
    ]
    .iter()
    .any(|version| !velnor_actions_contract::config::is_exact_semver(version))
    {
        return Err(internal("image_publication_lock_versions"));
    }
    if let Some(context) = runner_publication_dockerfile_context(image_name(candidate.profile)) {
        verify_dockerfile_context(source_files, context)?;
    }
    if candidate.lock.is_some() {
        verify_runner_lock_pins(lock_bytes, source_files)?;
    }
    if let Some(expected) = &candidate.lock
        && (expected.sha256 != hex(&Sha256::digest(lock_bytes))
            || expected.runner_version != lock.runner
            || expected.docker_version != lock.docker
            || expected.buildx_version != lock.buildx
            || expected.compose_version != lock.compose
            || expected.node_version != lock.node)
    {
        return Err(internal("image_publication_runner_lock_identity"));
    }
    if let Some(builder) = &candidate.buildx_builder {
        let version = super::validate::buildx_release_version(builder)
            .ok_or_else(|| internal("image_publication_job_buildx_identity"))?;
        if version != lock.buildx {
            return Err(internal("image_publication_job_buildx_identity"));
        }
    }
    Ok(())
}

fn verify_runner_lock_pins(
    bytes: &[u8],
    sources: &BTreeMap<String, Vec<u8>>,
) -> Result<(), OrchestratorError> {
    let lock: toml::Value = toml::from_str(
        std::str::from_utf8(bytes).map_err(|_| internal("image_publication_lock_utf8"))?,
    )
    .map_err(|_| internal("image_publication_lock_parse"))?;
    let get = |key: &str| {
        lock.get(key)
            .and_then(toml::Value::as_str)
            .ok_or_else(|| internal("image_publication_lock_pin"))
    };
    let runner = std::str::from_utf8(
        sources
            .get("images/runner/ubuntu-26.04/Dockerfile")
            .ok_or_else(|| internal("image_publication_dockerfile_missing"))?,
    )
    .map_err(|_| internal("image_publication_dockerfile_utf8"))?;
    let dind = std::str::from_utf8(
        sources
            .get("images/dind/Dockerfile")
            .ok_or_else(|| internal("image_publication_dockerfile_missing"))?,
    )
    .map_err(|_| internal("image_publication_dockerfile_utf8"))?;
    for dockerfile in [runner, dind] {
        let base = format!("FROM {}", get("base_image")?);
        if dockerfile.lines().filter(|line| *line == base).count() != 1 {
            return Err(internal("image_publication_lock_base"));
        }
    }
    for (prefix, versions, checksum) in [
        ("RUNNER", "runner_version", "runner_sha256"),
        ("DOCKER", "docker_version", "docker_sha256"),
        ("BUILDX", "buildx_version", "buildx_sha256"),
        ("COMPOSE", "compose_version", "compose_sha256"),
        ("NODE", "node_version", "node_sha256"),
    ] {
        for (suffix, key) in [("VERSION", versions), ("SHA256", checksum)] {
            let value = get(key)?;
            if suffix == "SHA256"
                && (value.len() != 64
                    || !value
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
            {
                return Err(internal("image_publication_lock_checksum"));
            }
            let line = format!("ARG {prefix}_{suffix}={value}");
            if runner
                .lines()
                .filter(|candidate| *candidate == line)
                .count()
                != 1
                || (prefix == "DOCKER"
                    && dind.lines().filter(|candidate| *candidate == line).count() != 1)
            {
                return Err(internal("image_publication_lock_argument"));
            }
        }
    }
    verify_lock_urls(&lock, runner, dind)?;
    for (actual, template) in [
        (
            runner,
            include_str!("../../../../images/runner/ubuntu-26.04/Dockerfile"),
        ),
        (dind, include_str!("../../../../images/dind/Dockerfile")),
    ] {
        if normalize_lock_lines(actual) != normalize_lock_lines(template) {
            return Err(internal("image_publication_lock_dockerfile"));
        }
    }
    verify_architecture_arms(&lock, runner, dind)
}

fn verify_architecture_arms(
    lock: &toml::Value,
    runner: &str,
    dind: &str,
) -> Result<(), OrchestratorError> {
    let get = |key: &str| {
        lock.get(key)
            .and_then(toml::Value::as_str)
            .ok_or_else(|| internal("image_publication_lock_architecture"))
    };
    let url = |key: &str, version_key: &str| -> Result<String, OrchestratorError> {
        let variable = format!("${{{}}}", version_key.to_ascii_uppercase());
        Ok(get(key)?.replace(get(version_key)?, &variable))
    };
    verify_arm64_urls(lock)?;
    let base = get("base_image_arm64_digest")?;
    let sums = [
        "runner_arm64_sha256",
        "docker_arm64_sha256",
        "buildx_arm64_sha256",
        "compose_arm64_sha256",
        "node_arm64_sha256",
    ];
    if !base.strip_prefix("sha256:").is_some_and(is_lower_sha256)
        || sums.iter().any(|key| !get(key).is_ok_and(is_lower_sha256))
    {
        return Err(internal("image_publication_lock_architecture"));
    }
    let runner_arms = [
        format!("amd64) runner_url=\"{}\" ;;", url("runner_asset_url", "runner_version")?),
        format!(
            "arm64) runner_url=\"{}\" RUNNER_SHA256=\"${{RUNNER_ARM64_SHA256}}\" ;;",
            url("runner_arm64_asset_url", "runner_version")?
        ),
        format!(
            "amd64) docker_url=\"{}\" compose_url=\"{}\" buildx_url=\"{}\" ;;",
            url("docker_archive_url", "docker_version")?,
            url("compose_asset_url", "compose_version")?,
            url("buildx_asset_url", "buildx_version")?
        ),
        format!(
            "arm64) docker_url=\"{}\" compose_url=\"{}\" buildx_url=\"{}\" DOCKER_SHA256=\"${{DOCKER_ARM64_SHA256}}\" COMPOSE_SHA256=\"${{COMPOSE_ARM64_SHA256}}\" BUILDX_SHA256=\"${{BUILDX_ARM64_SHA256}}\" ;;",
            url("docker_arm64_archive_url", "docker_version")?,
            url("compose_arm64_asset_url", "compose_version")?,
            url("buildx_arm64_asset_url", "buildx_version")?
        ),
        format!("amd64) node_url=\"{}\" ;;", url("node_archive_url", "node_version")?),
        format!(
            "arm64) node_url=\"{}\" NODE_SHA256=\"${{NODE_ARM64_SHA256}}\" ;;",
            url("node_arm64_archive_url", "node_version")?
        ),
    ];
    let dind_arms = [
        format!("amd64) docker_url=\"{}\" ;;", url("docker_archive_url", "docker_version")?),
        format!(
            "arm64) docker_url=\"{}\" DOCKER_SHA256=\"${{DOCKER_ARM64_SHA256}}\" ;;",
            url("docker_arm64_archive_url", "docker_version")?
        ),
    ];
    let arm_count = |dockerfile: &str, arm: &str| {
        dockerfile
            .lines()
            .filter(|line| line.trim().trim_end_matches('\\').trim_end() == arm)
            .count()
    };
    if runner_arms.iter().any(|arm| arm_count(runner, arm) != 1)
        || dind_arms.iter().any(|arm| arm_count(dind, arm) != 1)
    {
        return Err(internal("image_publication_lock_architecture"));
    }
    Ok(())
}

fn verify_arm64_urls(lock: &toml::Value) -> Result<(), OrchestratorError> {
    let get = |key: &str| {
        lock.get(key)
            .and_then(toml::Value::as_str)
            .ok_or_else(|| internal("image_publication_lock_architecture"))
    };
    for (key, template, version_key) in [
        (
            "runner_arm64_asset_url",
            "https://github.com/actions/runner/releases/download/v{v}/actions-runner-linux-arm64-{v}.tar.gz",
            "runner_version",
        ),
        (
            "docker_arm64_archive_url",
            "https://download.docker.com/linux/static/stable/aarch64/docker-{v}.tgz",
            "docker_version",
        ),
        (
            "buildx_arm64_asset_url",
            "https://github.com/docker/buildx/releases/download/v{v}/buildx-v{v}.linux-arm64",
            "buildx_version",
        ),
        (
            "compose_arm64_asset_url",
            "https://github.com/docker/compose/releases/download/v{v}/docker-compose-linux-aarch64",
            "compose_version",
        ),
        (
            "node_arm64_archive_url",
            "https://nodejs.org/dist/v{v}/node-v{v}-linux-arm64.tar.gz",
            "node_version",
        ),
    ] {
        if get(key)? != template.replace("{v}", get(version_key)?) {
            return Err(internal("image_publication_lock_architecture"));
        }
    }
    Ok(())
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn verify_lock_urls(lock: &toml::Value, runner: &str, dind: &str) -> Result<(), OrchestratorError> {
    let get = |key: &str| {
        lock.get(key)
            .and_then(toml::Value::as_str)
            .ok_or_else(|| internal("image_publication_lock_pin"))
    };
    for (key, template, version_key) in [
        (
            "runner_asset_url",
            "https://github.com/actions/runner/releases/download/v{v}/actions-runner-linux-x64-{v}.tar.gz",
            "runner_version",
        ),
        (
            "docker_archive_url",
            "https://download.docker.com/linux/static/stable/x86_64/docker-{v}.tgz",
            "docker_version",
        ),
        (
            "buildx_asset_url",
            "https://github.com/docker/buildx/releases/download/v{v}/buildx-v{v}.linux-amd64",
            "buildx_version",
        ),
        (
            "compose_asset_url",
            "https://github.com/docker/compose/releases/download/v{v}/docker-compose-linux-x86_64",
            "compose_version",
        ),
        (
            "node_archive_url",
            "https://nodejs.org/dist/v{v}/node-v{v}-linux-x64.tar.gz",
            "node_version",
        ),
    ] {
        let variable = format!("${{{}}}", version_key.to_ascii_uppercase());
        let docker_url = template.replace("{v}", &variable);
        if get(key)? != template.replace("{v}", get(version_key)?)
            || !runner.contains(&docker_url)
            || (key == "docker_archive_url" && !dind.contains(&docker_url))
        {
            return Err(internal("image_publication_lock_url"));
        }
    }
    Ok(())
}

pub(super) fn source_paths(
    profile: super::ImagePublicationProfile,
) -> Result<&'static [&'static str], OrchestratorError> {
    match profile {
        super::ImagePublicationProfile::Job => Ok(native_image_candidate_inputs()),
        super::ImagePublicationProfile::Runner
        | super::ImagePublicationProfile::Dind
        | super::ImagePublicationProfile::ResourceProbe => runner_publication_source_inputs()
            .map_err(|_| internal("image_publication_source_inventory")),
    }
}

fn verify_dockerfile_context(
    source_files: &BTreeMap<String, Vec<u8>>,
    context: RunnerPublicationDockerfileContext,
) -> Result<(), OrchestratorError> {
    let bytes = source_files
        .get(context.dockerfile)
        .ok_or_else(|| internal("image_publication_source_dockerfile_missing"))?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| internal("image_publication_source_dockerfile_utf8"))?;
    context.analyze(text).map(|_| ()).map_err(|error| {
        let code =
            match error {
                velnor_actions_contract::config::DockerfileProblem::UnsupportedRunnerSyntax(
                    value,
                ) if value == "role_context" => "image_publication_source_context",
                _ => "image_publication_source_dockerfile",
            };
        internal(code)
    })
}

fn normalize_lock_lines(dockerfile: &str) -> Vec<String> {
    dockerfile
        .lines()
        .map(|line| {
            if line.starts_with("FROM ") {
                return "FROM <locked>".to_owned();
            }
            for prefix in ["RUNNER", "DOCKER", "BUILDX", "COMPOSE", "NODE"] {
                for suffix in ["VERSION", "SHA256"] {
                    let key = format!("ARG {prefix}_{suffix}=");
                    if line.starts_with(&key) {
                        return format!("{key}<locked>");
                    }
                }
            }
            line.to_owned()
        })
        .collect()
}
