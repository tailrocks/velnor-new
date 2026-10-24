use crate::error::HostError;
use crate::journal::BootstrapContainerCreateClaim;
use crate::journal::connection::storage::in_transaction;
use crate::launch::bootstrap::BootstrapContainerRole;
use crate::worker::ResourceBudget;

use super::super::super::ConnectionJournal;

impl ConnectionJournal {
    /// Save the generated create request before it can reach the Docker daemon.
    pub(crate) async fn record_pre_jit_create_spec(
        &self,
        claim: &BootstrapContainerCreateClaim,
        budget: ResourceBudget,
        image_runtime_id: &str,
        image_source_sha: &str,
        spec_sha256: &str,
    ) -> Result<(), HostError> {
        if self.connection_id().as_str() != claim.connection_id()
            || !valid_hex(
                image_runtime_id.strip_prefix("sha256:").unwrap_or_default(),
                64,
            )
            || !valid_hex(image_source_sha, 40)
            || !valid_hex(spec_sha256, 64)
        {
            return Err(HostError::Identity);
        }
        let runner = budget.runner();
        let dind = budget.dind();
        let admission_memory = budget.admission_reserve().memory_bytes;
        let runner_cpu = exact_millicores(runner.nano_cpus)?;
        let dind_cpu = exact_millicores(dind.nano_cpus)?;
        let runner_memory = positive(runner.memory_bytes)?;
        let dind_memory = positive(dind.memory_bytes)?;
        let operation_role = match claim.role() {
            BootstrapContainerRole::Runner => "runner",
            BootstrapContainerRole::Dind => "dind",
        };
        let connection_id = claim.connection_id().to_owned();
        in_transaction(self.journal_ref(), async |connection| {
            self.validate_scope(connection).await?;
            let mut rows = connection
                .query(
                    "SELECT reservation.cpu_millicores, reservation.memory_bytes, reservation.state, intent.state, provisioning.operation_id, provisioning.engine_id, provisioning.docker_root_digest FROM capacity_reservations AS reservation JOIN connection_intents AS intent ON intent.id = reservation.intent_id AND intent.connection_id = reservation.connection_id JOIN capacity_reservation_provisioning AS provisioning ON provisioning.reservation_id = reservation.id WHERE reservation.id = ?1 AND reservation.connection_id = ?2 AND reservation.intent_id = ?3",
                    (claim.reservation_id(), connection_id.as_str(), claim.operation_id()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Identity)?;
            let pair_cpu: i64 = row.get(0).map_err(|_| HostError::Journal)?;
            let pair_memory: i64 = row.get(1).map_err(|_| HostError::Journal)?;
            let reservation_state: String = row.get(2).map_err(|_| HostError::Journal)?;
            let intent_state: String = row.get(3).map_err(|_| HostError::Journal)?;
            let operation_id: i64 = row.get(4).map_err(|_| HostError::Journal)?;
            let engine_id: String = row.get(5).map_err(|_| HostError::Journal)?;
            let root_digest: String = row.get(6).map_err(|_| HostError::Journal)?;
            if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
                || reservation_state != "reserved"
                || intent_state != "pending"
                || operation_id != claim.operation_id()
                || engine_id != claim.engine_id()
                || root_digest != claim.docker_root_digest()
                || pair_cpu != i64::try_from(runner_cpu.checked_add(dind_cpu).ok_or(HostError::Journal)?).map_err(|_| HostError::Journal)?
                || pair_memory != i64::try_from(runner_memory.checked_add(dind_memory).ok_or(HostError::Journal)?).map_err(|_| HostError::Journal)?
            {
                return Err(HostError::Identity);
            }
            let changed = connection
                .execute(
                    "INSERT INTO capacity_reservation_pre_jit_create_specs (reservation_id, role, connection_id, operation_id, engine_id, docker_root_digest, container_name, image_runtime_id, image_source_sha, runner_cpu_millicores, runner_memory_bytes, dind_cpu_millicores, dind_memory_bytes, admission_memory_bytes, spec_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    (claim.reservation_id(), operation_role, connection_id.as_str(), claim.operation_id(), claim.engine_id(), claim.docker_root_digest(), claim.container_name(), image_runtime_id, image_source_sha, to_sql(runner_cpu)?, to_sql(runner_memory)?, to_sql(dind_cpu)?, to_sql(dind_memory)?, to_sql(admission_memory)?, spec_sha256),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            if changed != 1 {
                return Err(HostError::Journal);
            }
            Ok(())
        })
        .await
    }
}

fn exact_millicores(nano_cpus: i64) -> Result<u64, HostError> {
    let nanos = u64::try_from(nano_cpus).map_err(|_| HostError::Identity)?;
    if nanos == 0 || nanos % 1_000_000 != 0 {
        return Err(HostError::Identity);
    }
    Ok(nanos / 1_000_000)
}

fn positive(value: i64) -> Result<u64, HostError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Identity)
}

fn to_sql(value: u64) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Journal)
}

fn valid_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
