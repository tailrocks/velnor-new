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
/// Returns [`HostError::ForbiddenMount`] when the volume name is empty.
pub fn runner_plan(private_volume: &str) -> Result<ContainerPlan, HostError> {
    if private_volume.is_empty() || private_volume.contains('/') {
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
        if forbidden_source(&mount.source) {
            return Err(HostError::ForbiddenMount);
        }
        if mount.target == "/var/run/docker.sock" && !mount.source.starts_with("volume:") {
            return Err(HostError::ForbiddenMount);
        }
    }
    Ok(())
}

fn forbidden_source(source: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "/Users/",
        "/home/",
        "/var/run/docker.sock",
        "ssh-agent",
        "keychain",
        "Application Support/Velnor",
    ];
    NEEDLES.iter().any(|needle| source.contains(needle))
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
