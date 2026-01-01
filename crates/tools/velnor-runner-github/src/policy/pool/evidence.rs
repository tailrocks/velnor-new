use std::time::SystemTime;

use super::super::types::{PolicyGap, PolicyMismatch, PoolBinding};
use super::EffectiveRoutingProof;

/// Pool policy assessment. A caller must also obtain its state-owned fenced
/// zero-assigned/running observation before admitting work.
#[derive(Debug, PartialEq, Eq)]
pub enum PoolAdmissionEvidence {
    /// Effective routing policy and exact pool identity were proven by a
    /// source-specific private proof.
    Verified(Box<VerifiedPoolPolicy>),
    /// Evidence is incomplete or only demonstrates consistency.
    Unknown(PolicyGap),
    /// Present evidence contradicts the expected binding or safety policy.
    Rejected(PolicyMismatch),
}

/// Private proof that an exact scale set is governed by an effective trusted
/// routing policy. No reader currently produces this proof: the known group
/// REST API result only proves metadata consistency and visibility.
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedPoolPolicy {
    pub(super) binding: PoolBinding,
    pub(super) policy_digest: String,
    pub(super) verified_at: SystemTime,
    pub(super) expires_at: SystemTime,
    pub(super) _routing_proof: EffectiveRoutingProof,
}

impl VerifiedPoolPolicy {
    /// Exact binding covered by the proof.
    #[must_use]
    pub const fn binding(&self) -> &PoolBinding {
        &self.binding
    }

    /// Trust policy digest covered by the proof.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    /// Time at which the proof was checked.
    #[must_use]
    pub const fn verified_at(&self) -> SystemTime {
        self.verified_at
    }

    /// Expiration time of the proof.
    #[must_use]
    pub const fn expires_at(&self) -> SystemTime {
        self.expires_at
    }
}
