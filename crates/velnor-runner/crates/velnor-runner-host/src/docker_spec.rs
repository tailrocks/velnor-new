// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use velnor_runner_core::runner_work_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub source: String,

    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerPlan {
    pub name: String,

    pub privileged: bool,

    pub platform: String,

    pub image: String,

    pub env: Vec<String>,

    pub cmd: Vec<String>,

    pub labels: Vec<String>,

    pub mounts: Vec<Mount>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteDecision {
    Delete,

    KeepForeign,

    NotDeleted,
}

pub(crate) const RUNNER_PLATFORM: &str = "linux/amd64";
pub(crate) const RUNNER_IMAGE: &str = "velnor-runner:ubuntu-26.04-2.338.0";
const ENTRYPOINT: &str = "/usr/local/bin/velnor-runner-entrypoint";
const SOCKET_TARGET: &str = "/run";

const HOST_NEEDLES: &[&str] = &[
    "ssh-agent",
    "ssh_auth_sock",
    "keychain",
    ".ssh",
    "application support/velnor",
    "orbstack",
    "com.tailrocks.velnor",
    "docker.sock",
];

pub fn runner_plan(private_volume: &str) -> Result<ContainerPlan, HostError> {
    if !private_volume_name(private_volume) {
        return Err(HostError::ForbiddenMount);
    }
    let work = format!("{private_volume}-work");
    Ok(ContainerPlan {
        name: format!("{private_volume}-runner"),
        privileged: false,
        platform: RUNNER_PLATFORM.to_owned(),
        image: RUNNER_IMAGE.to_owned(),
        env: Vec::new(),
        cmd: vec![ENTRYPOINT.to_owned()],
        labels: vec![
            "velnor.role=runner".to_owned(),
            format!("velnor.volume={private_volume}"),
            format!("velnor.worker={private_volume}"),
        ],
        mounts: vec![
            Mount {
                source: format!("volume:{private_volume}"),
                target: SOCKET_TARGET.to_owned(),
            },
            Mount {
                source: format!("volume:{work}"),
                target: runner_work_path(),
            },
        ],
    })
}

pub(crate) const SEED_VOLUME: &str = "volume:velnor-seed";

pub(crate) const SEED_TARGET: &str = "/opt/velnor/seed";

pub(crate) const ACTION_ARCHIVE_VOLUME: &str = "volume:velnor-action-archive";

pub(crate) const ACTION_ARCHIVE_TARGET: &str = "/opt/action-archive-cache";

pub(crate) fn runner_mounts(mounts: &[Mount]) -> Result<Vec<Mount>, HostError> {
    let mut out = mounts.to_vec();
    let seed = Mount {
        source: SEED_VOLUME.to_owned(),
        target: SEED_TARGET.to_owned(),
    };
    reject_mount(&seed)?;
    out.push(seed);
    let archive = Mount {
        source: ACTION_ARCHIVE_VOLUME.to_owned(),
        target: ACTION_ARCHIVE_TARGET.to_owned(),
    };
    reject_mount(&archive)?;
    out.push(archive);
    Ok(out)
}

pub fn audit_plan(plan: &ContainerPlan) -> Result<(), HostError> {
    if plan.privileged {
        return Err(HostError::PrivilegedRunner);
    }
    if shape_rejected(plan) || plan.env.iter().any(|entry| env_forbidden(entry)) {
        return Err(HostError::ForbiddenMount);
    }
    for mount in &plan.mounts {
        reject_mount(mount)?;
    }
    Ok(())
}

fn shape_rejected(plan: &ContainerPlan) -> bool {
    !private_volume_name(&plan.name)
        || plan.platform != RUNNER_PLATFORM
        || image_rejected(&plan.image)
        || contains_jit(&plan.env)
        || contains_jit(&plan.cmd)
        || contains_jit(&plan.labels)
}

fn image_rejected(image: &str) -> bool {
    image != RUNNER_IMAGE
}

fn contains_jit(items: &[String]) -> bool {
    items
        .iter()
        .any(|item| item.to_ascii_lowercase().contains("jitconfig"))
}

fn reject_mount(mount: &Mount) -> Result<(), HostError> {
    if forbidden_source(&mount.source) || bad_socket(mount) {
        Err(HostError::ForbiddenMount)
    } else {
        Ok(())
    }
}

fn bad_socket(mount: &Mount) -> bool {
    mentions_docker_sock(mount) && !socket_source_ok(&mount.source)
}

fn mentions_docker_sock(mount: &Mount) -> bool {
    let target = mount.target.to_ascii_lowercase();
    let source = mount.source.to_ascii_lowercase();
    target.contains("docker.sock") || source.contains("docker.sock")
}

fn socket_source_ok(source: &str) -> bool {
    source
        .strip_prefix("volume:")
        .is_some_and(private_volume_name)
}

#[must_use]
pub(crate) fn accepts_volume_name(name: &str) -> bool {
    private_volume_name(name)
}

fn private_volume_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => chars.all(volume_char),
        _ => false,
    }
}

fn volume_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-')
}

fn forbidden_source(source: &str) -> bool {
    host_exposure(&source.to_ascii_lowercase())
}

fn env_forbidden(entry: &str) -> bool {
    let folded = entry.to_ascii_lowercase();
    secret_key(env_key(&folded)) || folded.contains("jitconfig") || host_exposure(&folded)
}

fn env_key(entry: &str) -> &str {
    entry.split_once('=').map_or(entry, |(key, _)| key).trim()
}

fn secret_key(key: &str) -> bool {
    key == "ssh_auth_sock"
        || key == "ssh_agent_pid"
        || key == "docker_host"
        || key == "pat"
        || key.ends_with("_pat")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("credential")
        || key.contains("keychain")
}

fn host_exposure(folded: &str) -> bool {
    if HOST_NEEDLES.iter().any(|needle| folded.contains(needle)) {
        return true;
    }
    let path = folded.strip_prefix("volume:").unwrap_or(folded);
    contains_path(path, "/home")
        || contains_path(path, "/users")
        || contains_path(path, "/run")
        || contains_path(path, "/var/run")
        || contains_path(path, "/private/var/run")
}

fn contains_path(text: &str, path: &str) -> bool {
    let mut rest = text;
    while let Some(index) = rest.find(path) {
        let end = index + path.len();
        if path_boundary(rest, end) {
            return true;
        }
        rest = &rest[index + 1..];
    }
    false
}

fn path_boundary(text: &str, end: usize) -> bool {
    matches!(
        text.as_bytes().get(end),
        None | Some(b'/' | b'"' | b' ' | b',' | b'=' | b':')
    )
}

#[must_use]
pub fn delete_decision(owned_id: &str, observed: Option<&str>) -> DeleteDecision {
    match observed {
        Some(id) if !owned_id.is_empty() && id == owned_id => DeleteDecision::Delete,
        Some(_) => DeleteDecision::KeepForeign,
        None => DeleteDecision::NotDeleted,
    }
}

#[must_use]
pub fn plan_contains(plan: &ContainerPlan, canary: &str) -> bool {
    if canary.is_empty() {
        return false;
    }
    field_contains(&plan.env, canary)
        || field_contains(&plan.cmd, canary)
        || field_contains(&plan.labels, canary)
        || plan
            .mounts
            .iter()
            .any(|mount| mount.source.contains(canary) || mount.target.contains(canary))
}

fn field_contains(items: &[String], canary: &str) -> bool {
    items.iter().any(|item| item.contains(canary))
}
