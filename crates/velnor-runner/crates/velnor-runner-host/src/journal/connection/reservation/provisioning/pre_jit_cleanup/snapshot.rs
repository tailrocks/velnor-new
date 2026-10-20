use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::super::HoldContext;

pub(super) async fn snapshot_digest(
    connection: &turso::Connection,
    reservation_id: i64,
    context: &HoldContext,
) -> Result<String, HostError> {
    let mut digest = Sha256::new();
    digest.update(b"velnor-pre-jit-cleanup-snapshot-v1\0");
    for value in [
        context.operation_id.to_string(),
        context.scale_set_id.to_string(),
        context.engine_id.clone(),
        context.docker_root_digest.clone(),
        context.profile_generation_id.clone(),
        context.profile_digest.clone(),
        context.sample_generation_id.clone(),
        context.topology_version.to_string(),
    ] {
        append_value(&mut digest, &value)?;
    }
    append_rows(
        connection,
        reservation_id,
        "SELECT role, container_name, COALESCE(container_id, ''), create_state, start_state FROM capacity_reservation_bootstrap_containers WHERE reservation_id = ?1 ORDER BY role",
        &mut digest,
        5,
    )
    .await?;
    append_rows(
        connection,
        reservation_id,
        "SELECT role, container_name, CAST(client_api_major AS TEXT), CAST(client_api_minor AS TEXT), CAST(response_status AS TEXT) FROM capacity_reservation_pre_jit_create_responses WHERE reservation_id = ?1 ORDER BY role",
        &mut digest,
        5,
    )
    .await?;
    append_rows(
        connection,
        reservation_id,
        "SELECT role, connection_id, CAST(operation_id AS TEXT), engine_id, docker_root_digest, container_name, image_runtime_id, image_source_sha, CAST(runner_cpu_millicores AS TEXT), CAST(runner_memory_bytes AS TEXT), CAST(dind_cpu_millicores AS TEXT), CAST(dind_memory_bytes AS TEXT), CAST(admission_memory_bytes AS TEXT), spec_sha256 FROM capacity_reservation_pre_jit_create_specs WHERE reservation_id = ?1 ORDER BY role",
        &mut digest,
        14,
    )
    .await?;
    append_rows(
        connection,
        reservation_id,
        "SELECT role, pool_id, medium, filesystem_kind, inode_policy, COALESCE(planned_filesystem_instance_digest, ''), CAST(charge_bytes AS TEXT), CAST(charge_inodes AS TEXT), CAST(retained_memory_bytes AS TEXT) FROM capacity_reservation_provisioning_roles WHERE reservation_id = ?1 ORDER BY role",
        &mut digest,
        9,
    )
    .await?;
    append_rows(
        connection,
        reservation_id,
        "SELECT scope, pool_id, medium, filesystem_kind, inode_policy, CAST(max_bytes AS TEXT), CAST(max_inodes AS TEXT), CAST(reserve_bytes AS TEXT), CAST(reserve_inodes AS TEXT), COALESCE(filesystem_instance_digest, ''), COALESCE(CAST(total_bytes AS TEXT), ''), COALESCE(CAST(available_bytes AS TEXT), ''), COALESCE(CAST(total_inodes AS TEXT), ''), COALESCE(CAST(available_inodes AS TEXT), '') FROM capacity_reservation_provisioning_pools WHERE reservation_id = ?1 ORDER BY scope, pool_id",
        &mut digest,
        14,
    )
    .await?;
    append_rows(
        connection,
        reservation_id,
        "SELECT CAST(operation_id AS TEXT), CAST(generation AS TEXT), operation_key, helper_name, COALESCE(helper_id, ''), engine_id, docker_root_digest, image_runtime_id, image_source_sha, home_volume, work_volume, metadata_volume, COALESCE(metadata_volume_id, ''), create_dispatch_state, create_state, start_state, result_state, remove_state, abort_state, CAST(nano_cpus AS TEXT), CAST(memory_bytes AS TEXT), CAST(pids_limit AS TEXT), CAST(layout_version AS TEXT) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
        &mut digest,
        23,
    )
    .await?;
    append_rows(
        connection,
        reservation_id,
        "SELECT role, docker_name, COALESCE(docker_id, ''), create_state FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 ORDER BY role",
        &mut digest,
        4,
    )
    .await?;
    append_rows(
        connection,
        reservation_id,
        "SELECT network_name, COALESCE(network_id, ''), labels_json, create_state, remove_state FROM capacity_reservation_networks WHERE reservation_id = ?1",
        &mut digest,
        5,
    )
    .await?;
    Ok(hex(&digest.finalize()))
}

async fn append_rows(
    connection: &turso::Connection,
    reservation_id: i64,
    sql: &str,
    digest: &mut Sha256,
    columns: usize,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(sql, [reservation_id])
        .await
        .map_err(|_| HostError::Journal)?;
    let mut count = 0_u64;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        count = count.checked_add(1).ok_or(HostError::Journal)?;
        digest.update(count.to_be_bytes());
        for column in 0..columns {
            let value: String = row.get(column).map_err(|_| HostError::Journal)?;
            append_value(digest, &value)?;
        }
    }
    digest.update(count.to_be_bytes());
    Ok(())
}

fn append_value(digest: &mut Sha256, value: &str) -> Result<(), HostError> {
    let length = u64::try_from(value.len()).map_err(|_| HostError::Journal)?;
    digest.update(length.to_be_bytes());
    digest.update(value.as_bytes());
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        value.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    value
}
