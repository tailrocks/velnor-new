use crate::error::HostError;

use super::{PreJitBootstrapContainer, PreJitCreateSpec};

pub(super) async fn read_create_specs(
    connection: &turso::Connection,
    reservation_id: i64,
) -> Result<Vec<PreJitCreateSpec>, HostError> {
    let mut rows = connection
        .query(
            "SELECT role, container_name, image_runtime_id, image_source_sha, runner_cpu_millicores, runner_memory_bytes, dind_cpu_millicores, dind_memory_bytes, admission_memory_bytes, spec_sha256 FROM capacity_reservation_pre_jit_create_specs WHERE reservation_id = ?1 ORDER BY role",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut specs = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let runner_cpu = positive_u64(row.get(4).map_err(|_| HostError::Journal)?)?;
        let runner_memory = positive_u64(row.get(5).map_err(|_| HostError::Journal)?)?;
        let dind_cpu = positive_u64(row.get(6).map_err(|_| HostError::Journal)?)?;
        let dind_memory = positive_u64(row.get(7).map_err(|_| HostError::Journal)?)?;
        let admission_memory = positive_u64(row.get(8).map_err(|_| HostError::Journal)?)?;
        let operational_memory = admission_memory
            .checked_sub(crate::worker::PROBE_MEMORY_LIMIT_BYTES)
            .ok_or(HostError::Journal)?;
        let budget = crate::worker::ResourceBudgetConfig {
            runner_cpu_millicores: runner_cpu,
            runner_memory_bytes: runner_memory,
            dind_cpu_millicores: dind_cpu,
            dind_memory_bytes: dind_memory,
            operational_memory_reserve_bytes: operational_memory,
        };
        budget.validate()?;
        specs.push(PreJitCreateSpec {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            name: row.get(1).map_err(|_| HostError::Journal)?,
            image_runtime_id: row.get(2).map_err(|_| HostError::Journal)?,
            image_source_sha: row.get(3).map_err(|_| HostError::Journal)?,
            budget,
            spec_sha256: row.get(9).map_err(|_| HostError::Journal)?,
        });
    }
    Ok(specs)
}

pub(super) fn valid_create_specs(
    containers: &[PreJitBootstrapContainer],
    specs: &[PreJitCreateSpec],
) -> bool {
    let attempted: Vec<_> = containers
        .iter()
        .filter(|container| container.create_state() != "unclaimed")
        .collect();
    (1..=2).contains(&specs.len())
        && attempted.len() == specs.len()
        && specs.iter().all(|spec| {
            attempted
                .iter()
                .any(|container| container.role() == spec.role() && container.name() == spec.name())
        })
        && attempted.iter().all(|container| {
            specs
                .iter()
                .any(|spec| spec.role() == container.role() && spec.name() == container.name())
        })
        && specs
            .windows(2)
            .all(|pair| pair[0].budget() == pair[1].budget())
        && specs.iter().all(|spec| {
            valid_digest(spec.image_source_sha(), 40)
                && valid_digest(spec.spec_sha256(), 64)
                && spec.image_runtime_id().starts_with("sha256:")
                && valid_digest(spec.image_runtime_id().trim_start_matches("sha256:"), 64)
        })
}

fn positive_u64(value: i64) -> Result<u64, HostError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(HostError::Journal)
}

fn valid_digest(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
