// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::HostError;
use crate::journal::{StorageProfile, StorageProvisioningAdmission};
use crate::worker::ResourceBudget;

use super::{Observation, StartObservationWitness};

pub(crate) struct RecoveryObservation {
    observation: Observation,
    witness: StartObservationWitness,
    budget: ResourceBudget,
    charge: (u64, u64),
}

impl Observation {
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
    pub(crate) const fn charge(&self) -> (u64, u64) {
        self.charge
    }

    pub(crate) fn storage_admission(
        &self,
        profile: &StorageProfile,
        engine_id: &str,
        root_digest: &str,
    ) -> Option<StorageProvisioningAdmission> {
        self.observation
            .storage_provisioning_admission(profile, engine_id, root_digest)
    }

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
