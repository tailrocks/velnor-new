//! New resource sample for one charged reservation.

use crate::error::HostError;
use crate::journal::{StorageProfile, StorageProvisioningAdmission};
use crate::worker::ResourceBudget;

use super::{Observation, StartObservationWitness};

/// New guest resource sample for work with an existing capacity charge.
///
/// This token does not reserve more capacity.
pub(crate) struct RecoveryObservation {
    observation: Observation,
    witness: StartObservationWitness,
    budget: ResourceBudget,
    charge: (u64, u64),
}

impl Observation {
    /// Check this guest resource sample against the existing charge and selected engine.
    pub(crate) fn into_recovery_observation(
        self,
        budget: ResourceBudget,
        existing_charge: (u64, u64),
        expected_engine_id: &str,
        expected_root_digest: &str,
    ) -> Option<RecoveryObservation> {
        let pair = budget.pair();
        let reserve = budget.admission_reserve();
        let required_cpu = pair.cpu_millicores.checked_add(reserve.cpu_millicores)?;
        let required_memory = pair.memory_bytes.checked_add(reserve.memory_bytes)?;
        let recent = self.is_recent_at(std::time::Instant::now());
        if !recent
            || self.engine_id != expected_engine_id
            || self.docker_root_digest != expected_root_digest
            || existing_charge != (pair.cpu_millicores, pair.memory_bytes)
            || self.record.docker_root_free_bytes < 10 * 1024 * 1024 * 1024
            || self.effective_cpu_millicores < required_cpu
            || self.record.memory_available_bytes < required_memory
        {
            return None;
        }
        let witness = StartObservationWitness::from_observation(
            &self.record,
            self.engine_id.clone(),
            self.docker_root_digest.clone(),
            self.sample_generation_id.clone(),
            self.observed_at,
        );
        Some(RecoveryObservation {
            observation: self,
            witness,
            budget,
            charge: existing_charge,
        })
    }
}

impl RecoveryObservation {
    /// Return the sample ID.
    pub(crate) fn sample_generation_id(&self) -> &str {
        self.observation.sample_generation_id()
    }

    /// Return the sample used for recovery.
    pub(crate) fn witness(&self) -> &StartObservationWitness {
        &self.witness
    }

    /// Return the checked CPU and memory limits for both containers.
    pub(crate) const fn resource_budget(&self) -> ResourceBudget {
        self.budget
    }

    /// Return the existing reservation charge.
    pub(crate) const fn charge(&self) -> (u64, u64) {
        self.charge
    }

    /// Check the new sample against the current storage profile.
    pub(crate) fn storage_admission(
        &self,
        profile: &StorageProfile,
        engine_id: &str,
        root_digest: &str,
    ) -> Option<StorageProvisioningAdmission> {
        self.observation
            .storage_provisioning_admission(profile, engine_id, root_digest)
    }

    /// Use this sample for one recovery action only.
    pub(crate) fn consume_for_recovery(
        self,
        engine_id: &str,
        root_digest: &str,
        existing_charge: (u64, u64),
        admission_sample_generation_id: &str,
    ) -> Result<(StartObservationWitness, ResourceBudget), HostError> {
        if self.charge != existing_charge
            || !self.witness.is_fresh_for(
                engine_id,
                root_digest,
                &self.observation.sample_generation_id,
            )
        {
            return Err(HostError::Identity);
        }
        let mut witness = self.witness;
        witness.bind_recovery_sample(admission_sample_generation_id);
        Ok((witness, self.budget))
    }
}
