//! Readiness is not "a process exists".

use serde::{Deserialize, Serialize};

/// Numeric facts from one completed selected-guest resource probe.
///
/// This value is returned only after the probe sample is fresh and its owned
/// container has been removed. It does not change controller readiness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceProbeReport {
    /// Unique durable operation/sample identity.
    pub sample_generation_id: String,
    /// Guest CPU count reported by the selected Docker daemon.
    pub guest_cpus: u32,
    /// Guest physical memory total reported by the selected Docker daemon.
    pub memory_total_bytes: u64,
    /// Guest memory available to new work.
    pub memory_available_bytes: u64,
    /// One-minute guest load average rounded upward to thousandths.
    pub load_milli: u64,
    /// Total bytes on the filesystem containing Docker's root.
    pub docker_root_total_bytes: u64,
    /// Free bytes on the filesystem containing Docker's root.
    pub docker_root_free_bytes: u64,
    /// Unprivileged available bytes on the filesystem containing Docker's root.
    pub docker_root_available_bytes: u64,
    /// Raw filesystem inode total.
    pub docker_root_inode_total: u64,
    /// Raw free filesystem inodes.
    pub docker_root_inode_free: u64,
    /// Raw unprivileged available filesystem inodes.
    pub docker_root_inode_available: u64,
    /// Probe-classified filesystem inode policy.
    pub docker_root_inode_policy: ResourceProbeInodePolicy,
    /// Filesystem type name observed at the Docker-root mount.
    pub docker_root_fs_type_name: String,
    /// Filesystem ID observed at the Docker-root mount.
    pub docker_root_filesystem_id: u64,
    /// Optional memory pressure-stall average, in basis points.
    pub memory_psi_some_avg10_bps: Option<u64>,
}

/// Filesystem inode reporting mode measured by the resource probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceProbeInodePolicy {
    /// The filesystem reports finite inode counts.
    Finite,
    /// Btrfs reports zero inode counts because it allocates inodes dynamically.
    DynamicBtrfs,
    /// The probe cannot establish a filesystem inode limit.
    Unknown,
}

/// Controller readiness. `Ready` is set only by a bounded check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Docker engine is down.
    WaitingForEngine,
    /// Keychain or gh credential is missing.
    WaitingForCredentials,
    /// Journal, Docker, and GitHub are being reconciled.
    Reconciling,
    /// Bounded readiness succeeded.
    Ready,
    /// New admissions are stopped.
    Draining,
    /// A required dependency failed after startup.
    Degraded,
}

impl Readiness {
    /// Stable JSON token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WaitingForEngine => "waiting_for_engine",
            Self::WaitingForCredentials => "waiting_for_credentials",
            Self::Reconciling => "reconciling",
            Self::Ready => "ready",
            Self::Draining => "draining",
            Self::Degraded => "degraded",
        }
    }
}

/// Status document. No token field.
#[must_use]
pub fn status_json(readiness: Readiness) -> String {
    format!(r#"{{"state":"{}"}}"#, readiness.as_str())
}

/// Doctor document. `probe` does not upgrade an unready state.
#[must_use]
pub fn doctor_json(readiness: Readiness, probe: bool) -> String {
    let state = readiness.as_str();
    format!(r#"{{"state":"{state}","probe":{probe}}}"#)
}

/// Doctor document with the actual result of an optional completed probe.
///
/// The readiness state is preserved; a successful resource sample does not
/// prove that queue reconciliation or job admission is ready.
pub fn doctor_json_with_probe(
    readiness: Readiness,
    probe: Option<&ResourceProbeReport>,
) -> Result<String, serde_json::Error> {
    let probe = serde_json::to_string(&probe)?;
    Ok(format!(
        r#"{{"state":"{}","probe":{probe}}}"#,
        readiness.as_str()
    ))
}

/// Empty controller directory is not ready.
#[must_use]
pub const fn readiness_for_empty() -> Readiness {
    Readiness::WaitingForCredentials
}
