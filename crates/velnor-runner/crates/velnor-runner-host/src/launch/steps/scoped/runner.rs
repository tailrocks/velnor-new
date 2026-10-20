//! Physical reservation, JIT binding, and paired Docker start.

use sha2::{Digest, Sha256};

use crate::config::ApprovedJobAuthorityPolicy;
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, ConnectionJournal, RequestPhysicalReservationResult, ReservationRequest,
    StorageProvisioningAdmission,
};
#[cfg(test)]
use crate::journal::{ReservationResult, StorageAdmission};
use crate::launch::{SelectedEngineContext, resource_probe::StartObservationWitness};
use crate::scale_set::EnsureError;

use super::acquire;

mod start;
#[cfg(test)]
pub(super) use start::start;

pub(super) struct PhysicalIntent {
    pub(super) reservation_id: i64,
    intent_id: i64,
    request_id: Option<i64>,
    assignment: crate::journal::AssignmentIdentity,
    runner_name: String,
    sample_generation_id: String,
    observation_sample_generation_id: String,
    observation: StartObservationWitness,
    resource_budget: crate::worker::ResourceBudget,
    charge: (u64, u64),
    pub(super) authority_generation: u64,
    pub(super) authority_digest: String,
    origin: PhysicalIntentOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhysicalIntentOrigin {
    NewAcquire,
    RecoveredAcquire,
}

pub(crate) struct AcquiredProvisioning {
    physical: PhysicalIntent,
    request: crate::journal::AssignmentIdentity,
}

impl AcquiredProvisioning {
    pub(super) fn after_recorded_acquire(
        physical: PhysicalIntent,
        request: crate::journal::AssignmentIdentity,
    ) -> Self {
        Self { physical, request }
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.physical.reservation_id
    }

    pub(crate) const fn intent_id(&self) -> i64 {
        self.physical.intent_id
    }

    pub(crate) fn request(&self) -> &crate::journal::AssignmentIdentity {
        &self.request
    }

    pub(crate) fn physical_assignment(&self) -> &crate::journal::AssignmentIdentity {
        &self.physical.assignment
    }

    pub(crate) fn runner_name(&self) -> &str {
        &self.physical.runner_name
    }

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.physical.sample_generation_id
    }

    pub(crate) fn observation(&self) -> &StartObservationWitness {
        &self.physical.observation
    }

    pub(crate) const fn resource_budget(&self) -> crate::worker::ResourceBudget {
        self.physical.resource_budget
    }

    pub(crate) const fn charge(&self) -> (u64, u64) {
        self.physical.charge
    }

    pub(crate) const fn authority_generation(&self) -> u64 {
        self.physical.authority_generation
    }

    pub(crate) fn authority_digest(&self) -> &str {
        &self.physical.authority_digest
    }

    #[cfg(test)]
    pub(crate) fn mark_observation_stale_for_test(&mut self) {
        self.physical.observation.mark_stale_for_test();
    }
}

/// Keep the recorded `AcquireJobs` result so work can continue after restart.
#[must_use]
pub(crate) struct CheckedAcquiredRecovery(AcquiredProvisioning);

impl CheckedAcquiredRecovery {
    pub(crate) fn after_verified_recovery(
        candidate: crate::journal::AcquiredRecoveryCandidate,
        observation: crate::launch::RecoveryObservation,
        engine: &SelectedEngineContext,
    ) -> Result<Self, HostError> {
        if candidate.engine_id() != engine.engine_id()
            || candidate.docker_root_digest() != engine.docker_root_digest()
            || candidate.charge() != observation.charge()
        {
            return Err(HostError::Identity);
        }
        let sample_generation_id = candidate.sample_generation_id().to_owned();
        let observation_sample_generation_id = observation.sample_generation_id().to_owned();
        let (observation, resource_budget) = observation.consume_for_recovery(
            engine.engine_id(),
            engine.docker_root_digest(),
            candidate.charge(),
            candidate.sample_generation_id(),
        )?;
        let physical = PhysicalIntent {
            reservation_id: candidate.reservation_id(),
            intent_id: candidate.intent_id(),
            request_id: candidate.request().request_id(),
            assignment: candidate.physical_assignment().clone(),
            runner_name: candidate.runner_name().to_owned(),
            sample_generation_id,
            observation_sample_generation_id,
            observation,
            resource_budget,
            charge: candidate.charge(),
            authority_generation: candidate.policy_generation(),
            authority_digest: candidate.policy_digest().to_owned(),
            origin: PhysicalIntentOrigin::RecoveredAcquire,
        };
        Ok(Self(AcquiredProvisioning {
            physical,
            request: candidate.request().clone(),
        }))
    }

    pub(crate) fn into_acquired(self) -> AcquiredProvisioning {
        self.0
    }
}

pub(super) enum ReservationOutcome {
    AcquireClaimed(PhysicalIntent, acquire::ClaimedAcquire),
    Existing,
    AtCapacity,
    ReconcileRequired,
}

pub(super) async fn reserve_and_claim(
    scoped: &ConnectionJournal,
    permit: crate::launch::StartPermit,
    storage: StorageProvisioningAdmission,
    request: &crate::journal::AssignmentIdentity,
    authority_policy: &ApprovedJobAuthorityPolicy,
) -> Result<ReservationOutcome, EnsureError> {
    if authority_policy.connection_id() != scoped.connection_id() {
        return Err(EnsureError::Uncertain);
    }
    let nonce = stable_nonce(scoped, request).await?;
    let observation = permit.observation_witness();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let identity = scoped
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.clone(),
        })
        .await
        .map_err(super::super::map_journal)?;
    let runner_name = format!("v{nonce}");
    let (cpu_millicores, memory_bytes) = permit.charge();
    let resource_budget = permit.resource_budget();
    let engine_id = permit.engine_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let reservation = scoped
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &identity,
                engine_id: &engine_id,
                docker_root_digest: &docker_root_digest,
                cpu_millicores,
                memory_bytes,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            storage,
            request,
        )
        .await
        .map_err(super::super::map_journal)?;
    match reservation {
        RequestPhysicalReservationResult::AcquireClaimed {
            reservation_id,
            intent_id,
        } => {
            let claim = acquire::ClaimedAcquire::new(request.clone());
            Ok(ReservationOutcome::AcquireClaimed(
                PhysicalIntent {
                    reservation_id,
                    intent_id,
                    request_id: request.request_id(),
                    assignment: identity,
                    runner_name,
                    observation_sample_generation_id: sample_generation_id.clone(),
                    sample_generation_id,
                    observation,
                    resource_budget,
                    charge: (cpu_millicores, memory_bytes),
                    authority_generation: authority_policy.generation(),
                    authority_digest: authority_policy.policy_sha256().to_owned(),
                    origin: PhysicalIntentOrigin::NewAcquire,
                },
                claim,
            ))
        }
        RequestPhysicalReservationResult::Existing { .. } => Ok(ReservationOutcome::Existing),
        RequestPhysicalReservationResult::AtCapacity => Ok(ReservationOutcome::AtCapacity),
        RequestPhysicalReservationResult::ReconcileRequired { .. } => {
            Ok(ReservationOutcome::ReconcileRequired)
        }
    }
}

#[cfg(test)]
pub(super) async fn reserve(
    scoped: &ConnectionJournal,
    permit: crate::launch::StartPermit,
    storage: StorageAdmission,
) -> Result<PhysicalIntent, EnsureError> {
    let nonce = fresh_nonce()?;
    let observation = permit.observation_witness();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let identity = scoped
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.clone(),
        })
        .await
        .map_err(super::super::map_journal)?;
    let runner_name = format!("v{nonce}");
    let (cpu_millicores, memory_bytes) = permit.charge();
    let resource_budget = permit.resource_budget();
    let engine_id = permit.engine_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let reservation = scoped
        .try_reserve_with_storage(
            ReservationRequest {
                assignment: &identity,
                engine_id: &engine_id,
                docker_root_digest: &docker_root_digest,
                cpu_millicores,
                memory_bytes,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            storage,
        )
        .await
        .map_err(super::super::map_journal)?;
    let reservation_id = match reservation {
        ReservationResult::Reserved { reservation_id } => reservation_id,
        ReservationResult::AtCapacity => {
            return Err(EnsureError::Unexpected {
                status: 0,
                step: "host capacity",
            });
        }
        ReservationResult::Resumed { .. }
        | ReservationResult::ReconcileRequired { .. }
        | ReservationResult::Released { .. } => return Err(EnsureError::Uncertain),
    };
    let (intent_id, fresh) = scoped
        .begin_launch(&identity)
        .await
        .map_err(super::super::map_journal)?;
    if !fresh {
        return Err(EnsureError::Uncertain);
    }
    let (authority_generation, authority_digest) = scoped
        .test_active_authority_identity()
        .await
        .map_err(super::super::map_journal)?;
    Ok(PhysicalIntent {
        reservation_id,
        intent_id,
        request_id: None,
        assignment: identity,
        runner_name,
        observation_sample_generation_id: sample_generation_id.clone(),
        sample_generation_id,
        observation,
        resource_budget,
        charge: (cpu_millicores, memory_bytes),
        authority_generation,
        authority_digest,
        origin: PhysicalIntentOrigin::NewAcquire,
    })
}

async fn stable_nonce(
    scoped: &ConnectionJournal,
    request: &crate::journal::AssignmentIdentity,
) -> Result<String, EnsureError> {
    let binding = scoped
        .source_authorization_binding()
        .await
        .map_err(super::super::map_journal)?;
    let request_id = request.request_id().ok_or(EnsureError::Uncertain)?;
    if request.connection_id() != scoped.connection_id()
        || request.connection_id() != binding.connection_id()
        || request.scale_set_id() != binding.scale_set_id()
    {
        return Err(EnsureError::Uncertain);
    }
    let scale_set_id = request.scale_set_id().to_string();
    let request_id = request_id.to_string();
    let mut digest = Sha256::new();
    digest.update(b"velnor-physical-runner-nonce-v1\0");
    for field in [
        scoped.connection_id().as_str().as_bytes(),
        binding.binding_digest().as_bytes(),
        scale_set_id.as_bytes(),
        request_id.as_bytes(),
    ] {
        digest.update(
            u64::try_from(field.len())
                .map_err(|_| EnsureError::Uncertain)?
                .to_be_bytes(),
        );
        digest.update(field);
    }
    let bytes = digest.finalize();
    let mut nonce = String::with_capacity(32);
    for byte in bytes.iter().take(16) {
        use std::fmt::Write as _;
        write!(&mut nonce, "{byte:02x}").map_err(|_| EnsureError::Uncertain)?;
    }
    Ok(nonce)
}

fn fresh_nonce() -> Result<String, EnsureError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| EnsureError::Uncertain)?;
    let mut nonce = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut nonce, "{byte:02x}").map_err(|_| EnsureError::Uncertain)?;
    }
    Ok(nonce)
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
