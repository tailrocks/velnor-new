use crate::error::HostError;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;
use crate::worker::storage_initialization::VerifiedStorageInitializationReady;

use super::super::model::{self, BoundJobFactsSnapshot};
use super::JobFactsReadContext;
use super::roles::StorageFacts;
use super::state::LaunchFacts;
use super::values::nonnegative;

pub(super) async fn read_cgroups(
    connection: &turso::Connection,
    context: &JobFactsReadContext<'_>,
) -> Result<Vec<model::Cgroup>, HostError> {
    let mut cgroups = Vec::new();
    let mut rows = connection.query(
        "SELECT role, container_id, path, memory_max_bytes, memory_current_bytes, cpu_quota_micros, cpu_period_micros, pids_max FROM capacity_reservation_bound_cgroups WHERE reservation_id = ?1 ORDER BY role",
        [context.reservation_id],
    ).await.map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        cgroups.push(model::cgroup(model::CgroupParts {
            role: row.get(0).map_err(|_| HostError::Journal)?,
            container_id: row.get(1).map_err(|_| HostError::Journal)?,
            path: row.get(2).map_err(|_| HostError::Journal)?,
            memory_max_bytes: nonnegative(row.get(3).map_err(|_| HostError::Journal)?)?,
            memory_current_bytes: nonnegative(row.get(4).map_err(|_| HostError::Journal)?)?,
            cpu_quota_micros: nonnegative(row.get(5).map_err(|_| HostError::Journal)?)?,
            cpu_period_micros: nonnegative(row.get(6).map_err(|_| HostError::Journal)?)?,
            pids_max: nonnegative(row.get(7).map_err(|_| HostError::Journal)?)?,
        }));
    }
    if cgroups.len() != 2 {
        return Err(HostError::Journal);
    }
    Ok(cgroups)
}

pub(super) fn build_snapshot(
    context: &JobFactsReadContext<'_>,
    facts: LaunchFacts,
    storage: StorageFacts,
    cgroups: Vec<model::Cgroup>,
    ready: &VerifiedStorageInitializationReady,
    channel: &VerifiedProofChannelBinding,
) -> Result<BoundJobFactsSnapshot, HostError> {
    let guest = channel.job_facts_directory();
    let source = ready.job_facts();
    let document = model::new_document(model::DocumentParts {
        scope: model::scope(
            context.connection_id.to_owned(),
            facts.scale_set_id,
            facts.binding_digest,
        ),
        reservation: model::reservation(context.reservation_id, context.operation_id),
        runner: model::runner(
            context.runner_id,
            context.runner_name.to_owned(),
            facts.scale_set_id,
            context.runner_container_id.to_owned(),
            context.dind_container_id.to_owned(),
        ),
        engine: model::engine(
            context.engine_id.to_owned(),
            context.docker_root_digest.to_owned(),
        ),
        profile: model::profile(facts.profile_generation, facts.profile_digest),
        sample: model::sample(facts.sample_generation, facts.observed_at),
        instance_binding: model::instance_binding(model::InstanceBindingParts {
            connection_id: context.connection_id.to_owned(),
            scale_set_id: facts.scale_set_id,
            reservation_id: context.reservation_id,
            intent_id: context.operation_id,
            engine_id: context.engine_id.to_owned(),
            docker_root_digest: context.docker_root_digest.to_owned(),
            sample_generation_id: context.sample_generation_id.to_owned(),
            sample_observed_at_unix_ms: facts.observed_at,
            profile_generation_id: context.profile_generation_id.to_owned(),
            profile_digest: context.profile_digest.to_owned(),
            guest_boot_id: facts.guest_boot_id,
            runner_container_id: context.runner_container_id.to_owned(),
            dind_container_id: context.dind_container_id.to_owned(),
            roles: storage.roles,
            cgroups,
        }),
        cache: model::cache(
            "/__w/_temp/velnor/cache".to_owned(),
            "/__w/_temp/velnor/cache-staging".to_owned(),
            storage.cache_live.max_bytes(),
            storage.cache_live.max_inodes(),
            storage.cache_stage.max_bytes(),
            storage.cache_stage.max_inodes(),
        ),
        job_facts_mount: model::job_facts_mount(
            guest.target.clone(),
            model::linux_device_number(source.device().0, source.device().1),
            source.inode(),
            guest.mount_id,
            (guest.statx_device_major, guest.statx_device_minor),
            model::linux_device_number(guest.statx_device_major, guest.statx_device_minor),
            guest.inode,
        ),
    });
    document.encode(context.instance_binding_digest.to_owned(), guest.inode)
}
