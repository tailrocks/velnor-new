use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use velnor_runner_github::RunnerAbsenceReceipt;

use crate::{error::HostError, journal::PreparedRetirementRemoteAbsence, scale_set::EnsureError};

/// A host or remote failure without request bodies or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetirementRemoteAbsenceError {
    /// Local journal or keychain validation failed.
    Host(HostError),
    /// GitHub rejected the observation or returned an uncertain result.
    Remote(EnsureError),
}

impl From<HostError> for RetirementRemoteAbsenceError {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}

/// A receipt that the host creates after checking credentials and the exact repository identity.
pub(crate) struct AuthenticatedRetirementAbsence {
    prepared: PreparedRetirementRemoteAbsence,
    repository_id: i64,
    repository_owner: String,
    repository_name: String,
    repository_private: bool,
    service_base_sha256: String,
    operation_started_unix_ms: u128,
    operation_finished_unix_ms: u128,
    receipt: RunnerAbsenceReceipt,
}

pub(super) struct RemoteAbsenceEvidence {
    pub(super) repository_id: i64,
    pub(super) repository_owner: String,
    pub(super) repository_name: String,
    pub(super) service_base_sha256: String,
    pub(super) operation_started_unix_ms: u128,
    pub(super) operation_finished_unix_ms: u128,
    pub(super) receipt: RunnerAbsenceReceipt,
}

impl AuthenticatedRetirementAbsence {
    pub(super) fn verified(
        prepared: PreparedRetirementRemoteAbsence,
        evidence: RemoteAbsenceEvidence,
    ) -> Self {
        Self {
            prepared,
            repository_id: evidence.repository_id,
            repository_owner: evidence.repository_owner,
            repository_name: evidence.repository_name,
            repository_private: true,
            service_base_sha256: evidence.service_base_sha256,
            operation_started_unix_ms: evidence.operation_started_unix_ms,
            operation_finished_unix_ms: evidence.operation_finished_unix_ms,
            receipt: evidence.receipt,
        }
    }

    pub(crate) const fn prepared(&self) -> &PreparedRetirementRemoteAbsence {
        &self.prepared
    }

    pub(crate) const fn repository_id(&self) -> i64 {
        self.repository_id
    }

    pub(crate) fn repository_owner(&self) -> &str {
        &self.repository_owner
    }

    pub(crate) fn repository_name(&self) -> &str {
        &self.repository_name
    }

    pub(crate) const fn repository_private(&self) -> bool {
        self.repository_private
    }

    pub(crate) fn service_base_sha256(&self) -> &str {
        &self.service_base_sha256
    }

    pub(crate) const fn operation_started_unix_ms(&self) -> u128 {
        self.operation_started_unix_ms
    }

    pub(crate) const fn operation_finished_unix_ms(&self) -> u128 {
        self.operation_finished_unix_ms
    }

    pub(crate) const fn receipt(&self) -> &RunnerAbsenceReceipt {
        &self.receipt
    }
}

pub(super) fn deadline_before_lease(
    requested: Instant,
    lease_until: i64,
) -> Result<Instant, HostError> {
    let monotonic_anchor = Instant::now();
    let wall_clock_now = SystemTime::now();
    deadline_before_lease_at(requested, lease_until, wall_clock_now, monotonic_anchor)
}

pub(super) fn deadline_before_lease_at(
    requested: Instant,
    lease_until: i64,
    wall_clock_now: SystemTime,
    monotonic_anchor: Instant,
) -> Result<Instant, HostError> {
    let lease_deadline = u64::try_from(lease_until)
        .ok()
        .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
        .ok_or(HostError::Identity)?;
    let remaining = lease_deadline
        .duration_since(wall_clock_now)
        .map_err(|_| HostError::Identity)?;
    let lease_deadline = monotonic_anchor
        .checked_add(remaining)
        .ok_or(HostError::Identity)?;
    Ok(requested.min(lease_deadline))
}

pub(super) fn binding_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn unix_ms() -> Result<u128, HostError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|_| HostError::Identity)
}
