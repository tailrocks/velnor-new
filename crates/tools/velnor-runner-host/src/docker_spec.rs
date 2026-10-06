//! One Ubuntu 26.04 runner. Platform is `linux/amd64`. JIT stays off the plan.

use crate::error::HostError;
use velnor_runner_core::runner_work_path;

/// One mount. `source` is `volume:<name>` or a bind path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// Volume name or bind source.
    pub source: String,
    /// Container path.
    pub target: String,
}

/// Docker create projection the controller is allowed to send.
///
/// One runner. JIT is not a field: stdin feeds the entrypoint, not env, cmd, or labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerPlan {
    /// Deterministic per-worker container name used for crash recovery.
    pub name: String,
    /// Must stay false for the runner.
    pub privileged: bool,
    /// Requested OCI platform. Always `linux/amd64`, never the host or VM arch.
    pub platform: String,
    /// Ubuntu 26.04 runner image. Not an ARM tag.
    pub image: String,
    /// Env pairs. Must not carry JIT, host paths, or management tokens.
    pub env: Vec<String>,
    /// Runner invocation. Must not carry the JIT payload.
    pub cmd: Vec<String>,
    /// Labels. Must not carry JIT.
    pub labels: Vec<String>,
    /// Private volumes for this worker's socket and work tree.
    pub mounts: Vec<Mount>,
}

/// What a delete may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteDecision {
    /// Immutable id matched the owned id.
    Delete,
    /// Observed id is not the owned id. Leave it.
    KeepForeign,
    /// Observed id missing. Not proof of cleanup.
    NotDeleted,
}

const RUNNER_PLATFORM: &str = "linux/amd64";
const RUNNER_IMAGE: &str = "velnor-runner:ubuntu-26.04-2.337.0";
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

/// Build the runner plan. `private_volume` is this worker's socket volume.
///
/// The work tree uses `{private_volume}-work`. JIT is not accepted.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when the volume name is not one private name.
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

/// Reject privileged runners, non-amd64 platforms, and host or token exposure.
///
/// # Errors
///
/// Returns [`HostError::PrivilegedRunner`] when privileged.
/// Returns [`HostError::ForbiddenMount`] for host paths, tokens, `OrbStack`, the outer
/// socket, JIT in Docker config, an ARM image, or a platform other than `linux/amd64`.
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

/// Delete only when the immutable id matches. A missing id is not cleanup.
#[must_use]
pub fn delete_decision(owned_id: &str, observed: Option<&str>) -> DeleteDecision {
    match observed {
        Some(id) if !owned_id.is_empty() && id == owned_id => DeleteDecision::Delete,
        Some(_) => DeleteDecision::KeepForeign,
        None => DeleteDecision::NotDeleted,
    }
}

/// True only when `canary` leaked into env, cmd, labels, or mounts.
///
/// A canary that was never placed returns false. An empty canary is not a leak.
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

#[cfg(test)]
mod tests;
