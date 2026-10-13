//! Runner container plan. Privileged is false. JIT is not in metadata.

use crate::error::HostError;

/// One mount. `source` is `volume:<name>` or a bind path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// Volume name or bind source.
    pub source: String,
    /// Container path.
    pub target: String,
}

/// Docker create projection the controller is allowed to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerPlan {
    /// Must stay false for the runner.
    pub privileged: bool,
    /// Env pairs. Must not contain JIT.
    pub env: Vec<String>,
    /// Command. Must not contain JIT.
    pub cmd: Vec<String>,
    /// Labels. Must not contain JIT.
    pub labels: Vec<String>,
    /// Mounts.
    pub mounts: Vec<Mount>,
}

/// What a delete may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteDecision {
    /// Immutable id matched the owned id.
    Delete,
    /// Same name, different id. Leave it.
    KeepForeign,
    /// No body. Not success.
    NotDeleted,
}

/// Build the runner plan. The socket source is a private volume name.
///
/// # Errors
///
/// Returns [`HostError::ForbiddenMount`] when the volume name is not one private name.
pub fn runner_plan(private_volume: &str) -> Result<ContainerPlan, HostError> {
    if !private_volume_name(private_volume) {
        return Err(HostError::ForbiddenMount);
    }
    Ok(ContainerPlan {
        privileged: false,
        env: Vec::new(),
        cmd: vec!["/usr/local/bin/velnor-runner-entrypoint".to_owned()],
        labels: vec![
            "velnor.role=runner".to_owned(),
            format!("velnor.volume={private_volume}"),
        ],
        mounts: vec![Mount {
            source: format!("volume:{private_volume}"),
            target: "/var/run/docker.sock".to_owned(),
        }],
    })
}

/// Reject privileged runners and host mounts.
///
/// # Errors
///
/// Returns [`HostError::PrivilegedRunner`] or [`HostError::ForbiddenMount`].
pub fn audit_plan(plan: &ContainerPlan) -> Result<(), HostError> {
    if plan.privileged {
        return Err(HostError::PrivilegedRunner);
    }
    for mount in &plan.mounts {
        reject_mount(mount)?;
    }
    Ok(())
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

const FORBIDDEN_MOUNT_NEEDLES: &[&str] = &[
    "/users/",
    "/home/",
    "/var/run/docker.sock",
    "ssh-agent",
    "keychain",
    "application support/velnor",
];

fn forbidden_source(source: &str) -> bool {
    let folded = source.to_ascii_lowercase();
    if FORBIDDEN_MOUNT_NEEDLES
        .iter()
        .any(|needle| folded.contains(needle))
    {
        return true;
    }
    let path = folded.strip_prefix("volume:").unwrap_or(folded.as_str());
    path == "/users" || path == "/home"
}

/// Delete only when the immutable id matches. Missing is not success.
#[must_use]
pub fn delete_decision(owned_id: &str, observed: Option<&str>) -> DeleteDecision {
    match observed {
        Some(id) if !owned_id.is_empty() && id == owned_id => DeleteDecision::Delete,
        Some(_) => DeleteDecision::KeepForeign,
        None => DeleteDecision::NotDeleted,
    }
}

/// True when the canary appears in env, cmd, or labels.
#[must_use]
pub fn plan_contains(plan: &ContainerPlan, canary: &str) -> bool {
    plan.env.iter().any(|item| item.contains(canary))
        || plan.cmd.iter().any(|item| item.contains(canary))
        || plan.labels.iter().any(|item| item.contains(canary))
}
