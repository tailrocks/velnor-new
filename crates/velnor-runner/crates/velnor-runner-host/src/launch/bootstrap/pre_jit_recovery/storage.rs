use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    time::{Duration, Instant},
};

use bollard::{Docker, errors::Error as DockerError, models::Volume};
use serde::Serialize;

use crate::{
    docker_client::docker_deadline_after,
    error::HostError,
    journal::{ConnectionJournal, PreJitCleanupClaim, PreJitCleanupInventory, StorageRole},
    launch::{SelectedEngineContext, VerifiedWorkerImages, bootstrap::containers},
    worker::{WorkerIdentityNames, WorkerVolumeRole},
};

use super::bootstrap::verify_selection;
use super::{PreJitCleanupPhase, PreJitCleanupPhaseProof};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn remove_if_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    inventory: &PreJitCleanupInventory,
) -> Result<(), HostError> {
    let plan = inventory.storage_plan();
    verify_storage_plan(plan, claim)?;
    let private_volume = inventory
        .initializer()
        .home_volume()
        .strip_suffix("-home")
        .ok_or(HostError::Ownership)?;
    let names = WorkerIdentityNames::from_private_volume(private_volume)?;
    let expected = [
        (StorageRole::SharedRun, WorkerVolumeRole::Socket),
        (StorageRole::RunnerHome, WorkerVolumeRole::Home),
        (StorageRole::RunnerWork, WorkerVolumeRole::Work),
        (StorageRole::DindData, WorkerVolumeRole::DindData),
        (
            StorageRole::EngineMetadata,
            WorkerVolumeRole::RunnerMetadata,
        ),
    ];
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    verify_selection(docker, engine, images, claim, deadline).await?;
    let cleanup = StorageCleanup {
        docker,
        journal,
        engine,
        images,
        claim,
        deadline,
    };
    let observations = cleanup
        .inspect_and_remove(inventory, plan, &names, &expected)
        .await?;
    let selected = verify_selection(docker, engine, images, claim, deadline).await?;
    let evidence = serde_json::to_vec(&StoragePhaseEvidence {
        engine_id: selected.engine_id,
        docker_root_digest: selected.docker_root_digest,
        profile_generation_id: plan.profile_generation_id(),
        profile_digest: plan.profile_digest(),
        sample_generation_id: plan.sample_generation_id(),
        volumes: observations,
    })
    .map_err(|_| HostError::Identity)?;
    let proof = PreJitCleanupPhaseProof::after_observed_phase(
        claim,
        PreJitCleanupPhase::InitializerAbsent,
        PreJitCleanupPhase::StorageAbsent,
        &evidence,
    )?;
    journal.record_pre_jit_cleanup_phase(claim, &proof).await
}

fn verify_storage_plan(
    plan: &crate::journal::StorageProvisioningPlan,
    claim: &PreJitCleanupClaim,
) -> Result<(), HostError> {
    if plan.reservation_id() != claim.reservation_id()
        || plan.operation_id() != claim.operation_id()
        || plan.engine_id() != claim.engine_id()
        || plan.docker_root_digest() != claim.docker_root_digest()
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

struct StorageCleanup<'a> {
    docker: &'a Docker,
    journal: &'a ConnectionJournal,
    engine: &'a SelectedEngineContext,
    images: &'a VerifiedWorkerImages,
    claim: &'a PreJitCleanupClaim,
    deadline: Instant,
}

impl StorageCleanup<'_> {
    async fn inspect_and_remove(
        &self,
        inventory: &PreJitCleanupInventory,
        plan: &crate::journal::StorageProvisioningPlan,
        names: &WorkerIdentityNames,
        expected: &[(StorageRole, WorkerVolumeRole)],
    ) -> Result<Vec<VolumeObservation<'static>>, HostError> {
        if inventory.storage_objects().len() != expected.len() {
            return Err(HostError::Ownership);
        }
        let mut seen = BTreeSet::new();
        let mut observations = Vec::with_capacity(expected.len());
        for (role, volume_role) in expected {
            let role_name = role.as_str();
            if !seen.insert(role_name) {
                return Err(HostError::Ownership);
            }
            let rows = inventory
                .storage_objects()
                .iter()
                .filter(|row| row.role() == role_name)
                .collect::<Vec<_>>();
            let plans = plan
                .roles()
                .iter()
                .filter(|row| row.role() == *role)
                .collect::<Vec<_>>();
            if rows.len() != 1 || plans.len() != 1 {
                return Err(HostError::Ownership);
            }
            let row = rows[0];
            let role_plan = plans[0];
            let expected_name = names.volume_name(*volume_role);
            if row.name() != expected_name {
                return Err(HostError::Ownership);
            }
            observations.push(
                self.remove_volume(role, expected_name, row, role_plan)
                    .await?,
            );
        }
        Ok(observations)
    }

    async fn remove_volume(
        &self,
        role: &StorageRole,
        expected_name: String,
        row: &crate::journal::PreJitStorageObject,
        role_plan: &crate::journal::StorageProvisioningRole,
    ) -> Result<VolumeObservation<'static>, HostError> {
        let role_name = role.as_str();
        if row.create_state() == "unclaimed" {
            if row.id().is_some() {
                return Err(HostError::Ownership);
            }
            return Ok(volume_observation(
                role_name,
                expected_name,
                None,
                None,
                "unclaimed_not_inspected",
                role_plan,
            ));
        }
        if !matches!(row.create_state(), "requested" | "created" | "uncertain")
            || row.id().is_some_and(str::is_empty)
        {
            return Err(HostError::Ownership);
        }
        let labels = volume_labels(self.journal, self.claim, *role)?;
        let inspected = inspect_volume(self.docker, &expected_name, self.deadline).await?;
        let was_present = inspected.is_some();
        let observed_name = if let Some(volume) = inspected {
            self.remove_inspected_volume(role_plan, row, &expected_name, &labels, &volume)
                .await?;
            Some(expected_name.clone())
        } else {
            if let Some(saved_id) = row.id()
                && (saved_id != expected_name
                    || inspect_volume(self.docker, saved_id, self.deadline)
                        .await?
                        .is_some())
            {
                return Err(HostError::Ownership);
            }
            None
        };
        verify_selection(
            self.docker,
            self.engine,
            self.images,
            self.claim,
            self.deadline,
        )
        .await?;
        Ok(volume_observation(
            role_name,
            expected_name,
            row.id().map(str::to_owned),
            observed_name,
            if was_present {
                "removed_and_absent"
            } else {
                "already_absent"
            },
            role_plan,
        ))
    }

    async fn remove_inspected_volume(
        &self,
        role_plan: &crate::journal::StorageProvisioningRole,
        row: &crate::journal::PreJitStorageObject,
        expected_name: &str,
        labels: &HashMap<String, String>,
        volume: &Volume,
    ) -> Result<(), HostError> {
        if row.id().is_some_and(|id| id != volume.name) {
            return Err(HostError::Ownership);
        }
        containers::verify_pre_jit_volume(volume, expected_name, role_plan, labels)?;
        verify_selection(
            self.docker,
            self.engine,
            self.images,
            self.claim,
            self.deadline,
        )
        .await?;
        request(
            self.deadline,
            self.docker.remove_volume(
                expected_name,
                None::<bollard::query_parameters::RemoveVolumeOptions>,
            ),
        )
        .await?
        .map_err(|_| HostError::Docker)?;
        if inspect_volume(self.docker, expected_name, self.deadline)
            .await?
            .is_some()
        {
            return Err(HostError::Ownership);
        }
        Ok(())
    }
}

fn volume_observation(
    role: &'static str,
    name: String,
    saved_id: Option<String>,
    observed_name: Option<String>,
    outcome: &'static str,
    role_plan: &crate::journal::StorageProvisioningRole,
) -> VolumeObservation<'static> {
    VolumeObservation {
        role,
        name,
        saved_id,
        observed_name,
        outcome,
        charge_bytes: role_plan.charge_bytes(),
        charge_inodes: role_plan.charge_inodes(),
        medium: role_plan.medium().as_str(),
        filesystem_kind: role_plan.filesystem_kind().as_str(),
    }
}

fn volume_labels(
    journal: &ConnectionJournal,
    claim: &PreJitCleanupClaim,
    role: StorageRole,
) -> Result<HashMap<String, String>, HostError> {
    let connection_id = journal.connection_id().as_str();
    if connection_id.is_empty() || claim.reservation_id() <= 0 || claim.operation_id() <= 0 {
        return Err(HostError::Identity);
    }
    Ok(HashMap::from([
        ("velnor.connection".to_owned(), connection_id.to_owned()),
        (
            "velnor.reservation".to_owned(),
            claim.reservation_id().to_string(),
        ),
        (
            "velnor.operation".to_owned(),
            claim.operation_id().to_string(),
        ),
        ("velnor.role".to_owned(), role.as_str().to_owned()),
    ]))
}

async fn inspect_volume(
    docker: &Docker,
    name: &str,
    deadline: Instant,
) -> Result<Option<Volume>, HostError> {
    match request(deadline, docker.inspect_volume(name)).await? {
        Ok(volume) => Ok(Some(volume)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

async fn request<F: Future>(deadline: Instant, future: F) -> Result<F::Output, HostError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(HostError::Docker)?;
    docker_deadline_after(future, remaining.min(REQUEST_TIMEOUT)).await
}

#[derive(Serialize)]
struct StoragePhaseEvidence<'a> {
    engine_id: String,
    docker_root_digest: String,
    profile_generation_id: &'a str,
    profile_digest: &'a str,
    sample_generation_id: &'a str,
    volumes: Vec<VolumeObservation<'a>>,
}

#[derive(Serialize)]
struct VolumeObservation<'a> {
    role: &'a str,
    name: String,
    saved_id: Option<String>,
    observed_name: Option<String>,
    outcome: &'static str,
    charge_bytes: u64,
    charge_inodes: u64,
    medium: &'static str,
    filesystem_kind: &'static str,
}
