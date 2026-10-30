use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::ConnectionJournal;
use crate::launch::bootstrap::{JobFactsPublication, ListenerDeliveryReceipt};
use velnor_runner_github::RunnerReference;

use super::admission::sql_i64;
use super::{hold_context, hold_is_claimable, mark_hold_uncertain};

/// One durable publication attempt for the read-only job-facts file.
pub(crate) struct JobFactsPublicationClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    runner: RunnerReference,
    instance_binding_digest: String,
    profile_generation_id: String,
    profile_digest: String,
    sample_generation_id: String,
}

impl JobFactsPublicationClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }
}

/// One durable JIT delivery attempt after publication is recorded.
pub(crate) struct ListenerDeliveryClaim {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    instance_binding_digest: String,
    runner: RunnerReference,
}

impl ListenerDeliveryClaim {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn runner(&self) -> &RunnerReference {
        &self.runner
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }
}

impl ConnectionJournal {
    /// Claim one job-facts publication after the exact runner JIT is recorded.
    pub(crate) async fn claim_job_facts_publication(
        &self,
        reservation_id: i64,
    ) -> Result<Option<JobFactsPublicationClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
                return Ok(None);
            };
            if !hold_is_claimable(&context) {
                return Ok(None);
            }
            let Some(record) = bound_state(connection, reservation_id).await? else {
                return Ok(None);
            };
            if !record.resources_bound
                || record.bootstrap_state != "started"
                || record.jit_state != "bound"
                || record.publication_state != "not_requested"
                || record.delivery_state != "not_requested"
                || record.cleanup_proven
            {
                return Ok(None);
            }
            let runner = jit_runner(connection, &connection_id, context.operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET publication_state = 'requested' WHERE reservation_id = ?1 AND resources_bound = 1 AND bootstrap_start_state = 'started' AND jit_state = 'bound' AND publication_state = 'not_requested' AND delivery_state = 'not_requested' AND storage_cleanup_proven = 0",
                    [reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Ok(None);
            }
            Ok(Some(JobFactsPublicationClaim {
                connection_id: connection_id.clone(),
                reservation_id,
                operation_id: context.operation_id,
                engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                runner_container_id: record.runner_container_id,
                dind_container_id: record.dind_container_id,
                runner,
                instance_binding_digest: record.instance_binding_digest,
                profile_generation_id: context.profile_generation_id,
                profile_digest: context.profile_digest,
                sample_generation_id: context.sample_generation_id,
            }))
        })
        .await
    }

    /// Record the exact projection mount and file identity before JIT delivery.
    pub(crate) async fn record_job_facts_published(
        &self,
        claim: &JobFactsPublicationClaim,
        proof: &JobFactsPublication,
    ) -> Result<(), HostError> {
        validate_publication_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.sample_generation_id() != claim.sample_generation_id
            || proof.profile_generation_id() != claim.profile_generation_id
            || proof.profile_digest() != claim.profile_digest
            || proof.runner_container_id() != claim.runner_container_id
            || proof.dind_container_id() != claim.dind_container_id
            || !same_runner(proof.runner(), &claim.runner)
            || proof.instance_binding_digest() != claim.instance_binding_digest
            || proof.path() != "/opt/velnor/job-facts/storage-cache-admission.json"
            || !proof.read_only()
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_claim_state(connection, claim, "requested", "bound").await?;
            let mut rows = connection
                .query(
                    "SELECT runner_id, runner_name, runner_scale_set_id FROM connection_runner_jits WHERE intent_id = ?1 AND connection_id = ?2 AND state = 'bound'",
                    (claim.operation_id, claim.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
            if row.get::<i64>(0).map_err(|_| HostError::Journal)? != proof.runner().id
                || row.get::<String>(1).map_err(|_| HostError::Journal)? != proof.runner().name
                || row.get::<i64>(2).map_err(|_| HostError::Journal)? != proof.runner().runner_scale_set_id
            {
                return Err(HostError::Identity);
            }
            let device = proof.directory_identity();
            let filesystem_id = proof.filesystem_id();
            let packed_device = packed_device(device.0, device.1)?;
            connection
                .execute(
                    "INSERT INTO capacity_reservation_provisioning_publications (reservation_id, runner_id, runner_name, runner_scale_set_id, target, projection_sha256, directory_device_major, directory_device_minor, directory_inode, mount_id, filesystem_id_0, filesystem_id_1, filesystem_instance_digest, read_only) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1)",
                    turso::params![claim.reservation_id, proof.runner().id, proof.runner().name.as_str(), proof.runner().runner_scale_set_id, proof.path(), proof.projection_digest(), i64::from(device.0), i64::from(device.1), sql_i64(device.2)?, sql_i64(proof.mount_id())?, filesystem_id[0], filesystem_id[1], proof.filesystem_instance_digest()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET publication_state = 'published', projection_sha256 = ?1, publication_directory_device = ?2, publication_directory_inode = ?3, publication_mount_id = ?4 WHERE reservation_id = ?5 AND publication_state = 'requested' AND jit_state = 'bound' AND resources_bound = 1",
                    (proof.projection_digest(), packed_device, sql_i64(device.2)?, sql_i64(proof.mount_id())?, claim.reservation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET launch_phase = 'job_facts_published' WHERE id = ?1 AND connection_id = ?2 AND state = 'pending' AND cleanup_proven = 0 AND launch_phase = 'jit_bound'",
                    (claim.operation_id, claim.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 { Ok(()) } else { Err(HostError::Journal) }
        })
        .await
    }

    /// Keep full storage and CPU capacity after an uncertain publication.
    pub(crate) async fn mark_job_facts_publication_uncertain(
        &self,
        claim: &JobFactsPublicationClaim,
    ) -> Result<(), HostError> {
        validate_publication_claim(self, claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET publication_state = 'uncertain' WHERE reservation_id = ?1 AND publication_state = 'requested' AND resources_bound = 1 AND jit_state = 'bound'",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            mark_hold_uncertain(connection, &claim.connection_id, claim.reservation_id, claim.operation_id).await
        })
        .await
    }

    /// Claim the one-shot write of the already bound JIT to the held stdin.
    pub(crate) async fn claim_listener_delivery(
        &self,
        reservation_id: i64,
    ) -> Result<Option<ListenerDeliveryClaim>, HostError> {
        if reservation_id <= 0 {
            return Err(HostError::Identity);
        }
        let connection_id = self.connection_id().as_str().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
                return Ok(None);
            };
            if !hold_is_claimable(&context) { return Ok(None); }
            let Some(record) = bound_state(connection, reservation_id).await? else { return Ok(None); };
            if !record.resources_bound || record.jit_state != "bound" || record.publication_state != "published" || record.delivery_state != "not_requested" || record.cleanup_proven {
                return Ok(None);
            }
            let runner = jit_runner(connection, &connection_id, context.operation_id).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET delivery_state = 'requested' WHERE reservation_id = ?1 AND resources_bound = 1 AND jit_state = 'bound' AND publication_state = 'published' AND delivery_state = 'not_requested' AND storage_cleanup_proven = 0",
                    [reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Ok(None); }
            Ok(Some(ListenerDeliveryClaim {
                connection_id: connection_id.clone(), reservation_id,
                operation_id: context.operation_id, engine_id: context.engine_id,
                docker_root_digest: context.docker_root_digest,
                runner_container_id: record.runner_container_id,
                instance_binding_digest: record.instance_binding_digest,
                runner,
            }))
        })
        .await
    }

    /// Record that the exact JIT bytes reached the held Runner stdin and EOF closed it.
    pub(crate) async fn record_listener_delivered(
        &self,
        claim: &ListenerDeliveryClaim,
        proof: &ListenerDeliveryReceipt,
    ) -> Result<(), HostError> {
        validate_delivery_claim(self, claim)?;
        if proof.reservation_id() != claim.reservation_id
            || proof.operation_id() != claim.operation_id
            || proof.engine_id() != claim.engine_id
            || proof.docker_root_digest() != claim.docker_root_digest
            || proof.runner_container_id() != claim.runner_container_id
            || !same_runner(proof.runner(), &claim.runner)
            || !proof.delivered()
        {
            return Err(HostError::Identity);
        }
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            validate_delivery_state(connection, claim).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET delivery_state = 'delivered' WHERE reservation_id = ?1 AND delivery_state = 'requested' AND publication_state = 'published' AND jit_state = 'bound' AND resources_bound = 1",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            let changed = connection
                .execute(
                    "UPDATE connection_intents SET launch_phase = 'listener_delivered' WHERE id = ?1 AND connection_id = ?2 AND state = 'pending' AND cleanup_proven = 0 AND launch_phase = 'job_facts_published'",
                    (claim.operation_id, claim.connection_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed == 1 { Ok(()) } else { Err(HostError::Journal) }
        })
        .await
    }

    /// Keep capacity charged when only part of the JIT payload may have been written.
    pub(crate) async fn mark_listener_delivery_uncertain(
        &self,
        claim: &ListenerDeliveryClaim,
    ) -> Result<(), HostError> {
        validate_delivery_claim(self, claim)?;
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let changed = connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET delivery_state = 'uncertain' WHERE reservation_id = ?1 AND delivery_state = 'requested' AND publication_state = 'published' AND jit_state = 'bound'",
                    [claim.reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 { return Err(HostError::Journal); }
            mark_hold_uncertain(connection, &claim.connection_id, claim.reservation_id, claim.operation_id).await
        })
        .await
    }
}

struct BoundState {
    bootstrap_state: String,
    resources_bound: bool,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
    jit_state: String,
    publication_state: String,
    delivery_state: String,
    cleanup_proven: bool,
}

async fn bound_state(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Option<BoundState>, HostError> {
    let mut rows = connection
        .query(
            "SELECT bootstrap_start_state, resources_bound, runner_container_id, dind_container_id, instance_binding_digest, jit_state, publication_state, delivery_state, storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else { return Ok(None) };
    Ok(Some(BoundState {
        bootstrap_state: row.get(0).map_err(|_| HostError::Journal)?,
        resources_bound: row.get::<i64>(1).map_err(|_| HostError::Journal)? == 1,
        runner_container_id: row.get(2).map_err(|_| HostError::Journal)?,
        dind_container_id: row.get(3).map_err(|_| HostError::Journal)?,
        instance_binding_digest: row.get(4).map_err(|_| HostError::Journal)?,
        jit_state: row.get(5).map_err(|_| HostError::Journal)?,
        publication_state: row.get(6).map_err(|_| HostError::Journal)?,
        delivery_state: row.get(7).map_err(|_| HostError::Journal)?,
        cleanup_proven: row.get::<i64>(8).map_err(|_| HostError::Journal)? == 1,
    }))
}

async fn jit_runner(
    connection: &turso::Connection,
    connection_id: &str,
    operation_id: i64,
) -> Result<RunnerReference, HostError> {
    let mut rows = connection
        .query(
            "SELECT j.runner_id, j.runner_name, j.scale_set_id FROM connection_runner_jits AS j JOIN connection_intents AS i ON i.id = j.intent_id AND i.connection_id = j.connection_id WHERE j.intent_id = ?1 AND j.connection_id = ?2 AND j.state = 'bound' AND j.runner_id > 0 AND j.runner_name IS NOT NULL AND i.state = 'pending' AND i.cleanup_proven = 0",
            (operation_id, connection_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
    let runner = RunnerReference {
        id: row.get(0).map_err(|_| HostError::Journal)?,
        name: row.get(1).map_err(|_| HostError::Journal)?,
        runner_scale_set_id: row.get(2).map_err(|_| HostError::Journal)?,
    };
    if runner.id <= 0 || runner.runner_scale_set_id <= 0 || runner.name.is_empty() {
        return Err(HostError::Journal);
    }
    Ok(runner)
}

async fn validate_claim_state(
    connection: &turso::Connection,
    claim: &JobFactsPublicationClaim,
    publication_state: &str,
    jit_state: &str,
) -> Result<(), HostError> {
    let context = hold_context(connection, &claim.connection_id, claim.reservation_id)
        .await?.ok_or(HostError::Journal)?;
    if context.operation_id != claim.operation_id
        || context.engine_id != claim.engine_id
        || context.docker_root_digest != claim.docker_root_digest
        || !hold_is_claimable(&context)
    {
        return Err(HostError::Journal);
    }
    let state = bound_state(connection, claim.reservation_id).await?.ok_or(HostError::Journal)?;
    if state.bootstrap_state != "started"
        || !state.resources_bound
        || state.instance_binding_digest != claim.instance_binding_digest
        || state.jit_state != jit_state
        || state.publication_state != publication_state
        || state.cleanup_proven
        || state.runner_container_id != claim.runner_container_id
        || state.dind_container_id != claim.dind_container_id
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn validate_delivery_state(
    connection: &turso::Connection,
    claim: &ListenerDeliveryClaim,
) -> Result<(), HostError> {
    let context = hold_context(connection, &claim.connection_id, claim.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if context.operation_id != claim.operation_id
        || context.engine_id != claim.engine_id
        || context.docker_root_digest != claim.docker_root_digest
        || !hold_is_claimable(&context)
    {
        return Err(HostError::Journal);
    }
    let state = bound_state(connection, claim.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if state.bootstrap_state != "started"
        || !state.resources_bound
        || state.instance_binding_digest != claim.instance_binding_digest
        || state.jit_state != "bound"
        || state.publication_state != "published"
        || state.delivery_state != "requested"
        || state.cleanup_proven
        || state.runner_container_id != claim.runner_container_id
    {
        return Err(HostError::Journal);
    }
    if !same_runner(
        &jit_runner(connection, &claim.connection_id, claim.operation_id).await?,
        &claim.runner,
    ) {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_publication_claim(
    journal: &ConnectionJournal,
    claim: &JobFactsPublicationClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
        || claim.runner.id <= 0
        || claim.runner.runner_scale_set_id <= 0
    { Err(HostError::Identity) } else { Ok(()) }
}

fn validate_delivery_claim(
    journal: &ConnectionJournal,
    claim: &ListenerDeliveryClaim,
) -> Result<(), HostError> {
    if journal.connection_id().as_str() != claim.connection_id
        || claim.reservation_id <= 0
        || claim.operation_id <= 0
        || claim.runner.id <= 0
        || claim.runner.runner_scale_set_id <= 0
        || claim.runner_container_id.is_empty()
    { Err(HostError::Identity) } else { Ok(()) }
}

fn same_runner(left: &RunnerReference, right: &RunnerReference) -> bool {
    left.id == right.id
        && left.name == right.name
        && left.runner_scale_set_id == right.runner_scale_set_id
}

fn packed_device(major: u32, minor: u32) -> Result<i64, HostError> {
    let packed = (u64::from(major) << 32) | u64::from(minor);
    i64::try_from(packed).map_err(|_| HostError::Journal)
}

