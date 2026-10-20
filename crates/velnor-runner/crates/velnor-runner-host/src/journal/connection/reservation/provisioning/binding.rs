use std::collections::BTreeMap;

use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use crate::journal::connection::{ConnectionJournal, StorageRole};
use crate::launch::bootstrap::VerifiedStorageResourceBinding;

use super::{HoldContext, hold_context};

mod attachments;
mod root_backing;

pub(super) async fn bind(
    journal: &ConnectionJournal,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if reservation_id <= 0 || binding.reservation_id() != reservation_id {
        return Err(HostError::Identity);
    }
    let connection_id = journal.connection_id().as_str().to_owned();
    in_transaction(journal.journal_ref(), async |connection| {
        journal.validate_scope(connection).await?;
        let Some(context) = hold_context(connection, &connection_id, reservation_id).await? else {
            return Err(HostError::Journal);
        };
        if !super::hold_is_claimable(&context)
            || context.operation_id != binding.operation_id()
            || context.engine_id != binding.engine_id()
            || context.docker_root_digest != binding.docker_root_digest()
            || context.profile_generation_id != binding.profile_generation_id()
            || context.profile_digest != binding.profile_digest()
            || context.sample_generation_id != binding.sample_generation_id()
        {
            return Err(HostError::Identity);
        }
        let Some(profile) = super::super::storage_budget::profile_generation(
            connection,
            &context.profile_generation_id,
        )
        .await?
        else {
            return Err(HostError::Journal);
        };
        profile.validate_provisioning_contract()?;
        validate_hold_objects(connection, reservation_id, binding).await?;
        validate_binding(
            &profile,
            &context,
            connection,
            &connection_id,
            reservation_id,
            binding,
        )
        .await?;

        attachments::insert_bound_rows(connection, reservation_id, binding).await?;
        super::cleanup::insert_observer_witness(
            connection,
            &connection_id,
            reservation_id,
            binding,
        )
        .await?;
        let changed = connection
            .execute(
                "UPDATE capacity_reservation_provisioning_state SET resources_bound = 1, instance_binding_digest = ?1, guest_boot_id = ?2 WHERE reservation_id = ?3 AND bootstrap_start_state = 'started' AND runner_container_id = ?4 AND dind_container_id = ?5 AND resources_bound = 0 AND instance_binding_digest IS NULL AND jit_state = 'not_requested'",
                (
                    binding.instance_binding_digest(),
                    binding.guest_boot_id(),
                    reservation_id,
                    binding.runner_container_id(),
                    binding.dind_container_id(),
                ),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed != 1 {
            return Err(HostError::Journal);
        }
        let changed = connection
            .execute(
                "UPDATE connection_intents SET launch_phase = 'resources_bound' WHERE id = ?1 AND connection_id = ?2 AND kind = 'launch' AND state = 'pending' AND cleanup_proven = 0 AND launch_phase = 'bootstrap_started'",
                (context.operation_id, connection_id.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(HostError::Journal)
        }
    })
    .await
}

pub(super) async fn resources_bound(
    connection: &turso::Connection,
    connection_id: &str,
    intent_id: i64,
) -> Result<bool, HostError> {
    let expected_attachments = StorageRole::PHYSICAL
        .iter()
        .map(|role| role.attachments().len())
        .sum::<usize>();
    let expected_attachments =
        i64::try_from(expected_attachments).map_err(|_| HostError::Journal)?;
    let expected_metadata_attachments =
        i64::try_from(StorageRole::EngineMetadata.attachments().len())
            .map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT 1 FROM capacity_reservation_provisioning AS p JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = p.reservation_id JOIN capacity_reservations AS r ON r.id = p.reservation_id JOIN connection_intents AS i ON i.id = p.operation_id AND i.connection_id = r.connection_id WHERE p.operation_id = ?1 AND r.connection_id = ?2 AND r.state = 'reserved' AND i.state = 'pending' AND i.cleanup_proven = 0 AND i.launch_phase = 'resources_bound' AND s.bootstrap_start_state = 'started' AND s.resources_bound = 1 AND s.jit_state = 'not_requested' AND s.storage_cleanup_proven = 0 AND (SELECT COUNT(*) FROM capacity_reservation_root_backing_mappings AS m WHERE m.reservation_id = p.reservation_id AND m.connection_id = r.connection_id AND m.operation_id = p.operation_id AND m.role IN ('runner-root', 'dind-root')) = 2 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS a WHERE a.reservation_id = p.reservation_id) = ?3 AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_attachments AS a WHERE a.reservation_id = p.reservation_id AND a.role = 'engine-metadata') = ?4 AND EXISTS (SELECT 1 FROM capacity_reservation_bound_storage_paths AS x JOIN capacity_reservation_bound_storage_attachments AS a ON a.reservation_id = x.reservation_id AND a.role = x.role AND a.observer = x.observer AND a.mount_target = x.mount_target WHERE x.reservation_id = p.reservation_id AND x.role = 'runner-home' AND x.observer = 'runner' AND x.mount_target = '/opt/velnor/source-proof/inbox' AND x.volume_subpath IS NOT NULL AND x.volume_subpath = 'source-proof/inbox' AND a.read_only = 0) AND NOT EXISTS (SELECT 1 FROM capacity_reservation_bound_storage_paths AS x WHERE x.reservation_id = p.reservation_id AND x.role = 'engine-metadata' AND x.observer = 'runner' AND x.mount_target = '/opt/velnor/source-proof/inbox') AND (SELECT COUNT(*) FROM capacity_reservation_bound_storage_paths AS x WHERE x.reservation_id = p.reservation_id) = ?3 AND EXISTS (SELECT 1 FROM capacity_reservation_bound_storage_paths AS x JOIN capacity_reservation_bound_storage_attachments AS a ON a.reservation_id = x.reservation_id AND a.role = x.role AND a.observer = x.observer AND a.mount_target = x.mount_target WHERE x.reservation_id = p.reservation_id AND x.role = 'engine-metadata' AND x.observer = 'dind' AND x.mount_target = '/.velnor-runtime-identity' AND a.source_target IS NULL AND x.volume_subpath = 'runtime-identity' AND a.read_only = 1 AND a.container_id = s.dind_container_id) AND EXISTS (SELECT 1 FROM capacity_reservation_bound_storage_paths AS x JOIN capacity_reservation_bound_storage_attachments AS a ON a.reservation_id = x.reservation_id AND a.role = x.role AND a.observer = x.observer AND a.mount_target = x.mount_target WHERE x.reservation_id = p.reservation_id AND x.role = 'engine-metadata' AND x.observer = 'dind' AND x.mount_target = '/opt/velnor/job-facts' AND a.source_target IS NULL AND x.volume_subpath = 'job-facts' AND a.read_only = 1 AND a.container_id = s.dind_container_id)",
            (intent_id, connection_id, expected_attachments, expected_metadata_attachments),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(rows.next().await.map_err(|_| HostError::Journal)?.is_some())
}

async fn validate_hold_objects(
    connection: &turso::Connection,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    let names = reservation_names(connection, reservation_id).await?;
    let mut rows = connection
        .query(
        "SELECT role, docker_name, create_state, docker_id FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut objects = BTreeMap::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let role: String = row.get(0).map_err(|_| HostError::Journal)?;
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        let state: String = row.get(2).map_err(|_| HostError::Journal)?;
        let id: Option<String> = row.get(3).map_err(|_| HostError::Journal)?;
        let expected_name = match role.as_str() {
            "shared-run" => names.volume_name(crate::worker::WorkerVolumeRole::Socket),
            "runner-home" => names.volume_name(crate::worker::WorkerVolumeRole::Home),
            "runner-work" => names.volume_name(crate::worker::WorkerVolumeRole::Work),
            "dind-data" => names.volume_name(crate::worker::WorkerVolumeRole::DindData),
            "engine-metadata" => names.volume_name(crate::worker::WorkerVolumeRole::RunnerMetadata),
            _ => return Err(HostError::Journal),
        };
        if state != "created" || name != expected_name || id.as_deref() != Some(name.as_str()) {
            return Err(HostError::Journal);
        }
        objects.insert(role, id.ok_or(HostError::Journal)?);
    }
    if objects.len() != 5 {
        return Err(HostError::Journal);
    }

    let mut rows = connection
        .query(
            "SELECT b.role, b.container_name, b.container_id, b.create_state, b.start_state, s.bootstrap_start_state, s.runner_container_id, s.dind_container_id FROM capacity_reservation_bootstrap_containers AS b JOIN capacity_reservation_provisioning_state AS s ON s.reservation_id = b.reservation_id WHERE b.reservation_id = ?1 ORDER BY b.role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut containers = BTreeMap::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let role: String = row.get(0).map_err(|_| HostError::Journal)?;
        let name: String = row.get(1).map_err(|_| HostError::Journal)?;
        let id: Option<String> = row.get(2).map_err(|_| HostError::Journal)?;
        let create_state: String = row.get(3).map_err(|_| HostError::Journal)?;
        let start_state: String = row.get(4).map_err(|_| HostError::Journal)?;
        let bootstrap_state: String = row.get(5).map_err(|_| HostError::Journal)?;
        let runner_id: Option<String> = row.get(6).map_err(|_| HostError::Journal)?;
        let dind_id: Option<String> = row.get(7).map_err(|_| HostError::Journal)?;
        if create_state != "created" || start_state != "started" || bootstrap_state != "started" {
            return Err(HostError::Journal);
        }
        let id = id.ok_or(HostError::Journal)?;
        if match role.as_str() {
            "runner" => {
                name == names.container_name(crate::worker::WorkerContainerRole::Runner)
                    && id == binding.runner_container_id()
                    && runner_id.as_deref() == Some(id.as_str())
            }
            "dind" => {
                name == names.container_name(crate::worker::WorkerContainerRole::Dind)
                    && id == binding.dind_container_id()
                    && dind_id.as_deref() == Some(id.as_str())
            }
            _ => false,
        } {
            containers.insert(role, id);
        } else {
            return Err(HostError::Identity);
        }
    }
    if containers.len() != 2 {
        return Err(HostError::Journal);
    }
    Ok(())
}

async fn reservation_names(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<crate::worker::WorkerIdentityNames, HostError> {
    let mut rows = connection
        .query(
            "SELECT i.runner_name FROM connection_intents AS i JOIN capacity_reservations AS r ON r.intent_id = i.id AND r.connection_id = i.connection_id WHERE r.id = ?1 AND i.kind = 'launch'",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let name: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    let nonce = name.strip_prefix('v').ok_or(HostError::Journal)?;
    crate::worker::WorkerIdentityNames::from_nonce(nonce)
}

async fn validate_binding(
    profile: &super::super::storage_budget::StorageProfile,
    context: &HoldContext,
    connection: &turso::Connection,
    connection_id: &str,
    reservation_id: i64,
    binding: &VerifiedStorageResourceBinding,
) -> Result<(), HostError> {
    if binding.engine_id() != context.engine_id
        || binding.docker_root_digest() != context.docker_root_digest
        || binding.sample_generation_id() != context.sample_generation_id
        || binding.profile_generation_id() != context.profile_generation_id
        || binding.profile_digest() != context.profile_digest
    {
        return Err(HostError::Identity);
    }
    root_backing::validate_roles(profile, connection, connection_id, reservation_id, binding)
        .await?;
    attachments::validate_cgroups(connection, reservation_id, binding).await
}
