//! Store and validate controller readiness evidence.

use std::time::{Duration, Instant};

use crate::error::HostError;
use crate::journal::ReadinessCapacityUsage;
use crate::launch::{ReadinessSample, SelectedEngineContext};

use super::ControlState;
use super::session_polls::SessionPollReceipt;

#[derive(Clone, Debug)]
pub(super) struct ControllerReadinessProof {
    config_sha256: String,
    engine_id: String,
    docker_root_digest: String,
    journal_instance_id: String,
    journal_revision: u64,
    sample_generation: String,
    sample_observed_at: Instant,
    collection_started_at: Instant,
    collection_finished_at: Instant,
    capacity: ReadinessCapacityUsage,
    sample_cpu_millicores: u64,
    sample_memory_total_bytes: u64,
    sample_memory_available_bytes: u64,
    inventory_sha256: String,
    poll_receipts: Vec<SessionPollReceipt>,
}

impl ControlState {
    pub(crate) fn readiness_sample(
        &self,
        selected: &SelectedEngineContext,
        now: Instant,
    ) -> Result<Option<ReadinessSample>, HostError> {
        let sample = self
            .readiness_sample
            .read()
            .map_err(|_| HostError::Journal)?
            .clone();
        Ok(sample.filter(|sample| sample.matches_engine(selected) && sample.is_recent(now)))
    }

    pub(crate) fn cache_readiness_sample(&self, sample: ReadinessSample) -> Result<(), HostError> {
        *self
            .readiness_sample
            .write()
            .map_err(|_| HostError::Journal)? = Some(sample);
        Ok(())
    }

    pub(crate) fn clear_readiness_sample(&self) -> Result<(), HostError> {
        self.readiness_sample
            .write()
            .map_err(|_| HostError::Journal)?
            .take();
        Ok(())
    }

    pub(crate) fn publish_readiness(
        &self,
        config_sha256: String,
        engine_id: String,
        docker_root_digest: String,
        journal_instance_id: String,
        journal_revision: u64,
        sample_generation: String,
        sample_observed_at: Instant,
        collection_started_at: Instant,
        collection_finished_at: Instant,
        capacity: ReadinessCapacityUsage,
        sample_cpu_millicores: u64,
        sample_memory_total_bytes: u64,
        sample_memory_available_bytes: u64,
        inventory_sha256: String,
        poll_receipts: Vec<SessionPollReceipt>,
    ) -> Result<(), HostError> {
        if !self.admission_ready()
            || config_sha256.len() != 64
            || engine_id.trim().is_empty()
            || docker_root_digest.len() != 64
            || journal_instance_id.trim().is_empty()
            || journal_revision == 0
            || sample_generation.trim().is_empty()
            || collection_started_at > collection_finished_at
            || sample_observed_at > collection_finished_at
            || !capacity.valid_for_guest(
                sample_cpu_millicores,
                sample_memory_total_bytes,
                sample_memory_available_bytes,
            )
            || !lower_hex_digest(&inventory_sha256)
            || poll_receipts.is_empty()
        {
            return Err(HostError::Identity);
        }
        let current = self
            .complete_session_poll_receipts()?
            .ok_or(HostError::Identity)?;
        if current != poll_receipts {
            return Err(HostError::Identity);
        }
        *self.readiness.write().map_err(|_| HostError::Journal)? = Some(ControllerReadinessProof {
            config_sha256,
            engine_id,
            docker_root_digest,
            journal_instance_id,
            journal_revision,
            sample_generation,
            sample_observed_at,
            collection_started_at,
            collection_finished_at,
            capacity,
            sample_cpu_millicores,
            sample_memory_total_bytes,
            sample_memory_available_bytes,
            inventory_sha256,
            poll_receipts,
        });
        Ok(())
    }

    pub(crate) fn clear_readiness(&self) -> Result<(), HostError> {
        self.readiness
            .write()
            .map_err(|_| HostError::Journal)?
            .take();
        Ok(())
    }

    pub(crate) fn readiness_matches(
        &self,
        config_sha256: &str,
        engine_id: &str,
        docker_root_digest: &str,
        journal_instance_id: &str,
        journal_revision: u64,
        now: Instant,
    ) -> Result<bool, HostError> {
        if !self.admission_ready() {
            return Ok(false);
        }
        let Some(proof) = self
            .readiness
            .read()
            .map_err(|_| HostError::Journal)?
            .clone()
        else {
            return Ok(false);
        };
        let Some(receipts) = self.complete_session_poll_receipts()? else {
            return Ok(false);
        };
        Ok(proof.config_sha256 == config_sha256
            && proof.engine_id == engine_id
            && proof.docker_root_digest == docker_root_digest
            && proof.journal_instance_id == journal_instance_id
            && proof.journal_revision == journal_revision
            && now
                .checked_duration_since(proof.sample_observed_at)
                .is_some_and(|age| age <= Duration::from_secs(30))
            && proof.sample_observed_at <= proof.collection_finished_at
            && proof.collection_started_at <= proof.collection_finished_at
            && now
                .checked_duration_since(proof.collection_finished_at)
                .is_some_and(|age| age <= Duration::from_secs(5))
            && proof.capacity.valid_for_guest(
                proof.sample_cpu_millicores,
                proof.sample_memory_total_bytes,
                proof.sample_memory_available_bytes,
            )
            && lower_hex_digest(&proof.inventory_sha256)
            && !proof.sample_generation.is_empty()
            && proof.poll_receipts == receipts)
    }
}

fn lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
