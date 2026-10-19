use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::read_transaction;
use crate::worker::execution_authorization::VerifiedProofChannelBinding;
use crate::worker::storage_initialization::VerifiedStorageInitializationReady;

use super::model::BoundJobFactsSnapshot;

mod attachments;
mod projection;
mod roles;
mod state;
mod values;

/// Values copied from one journal-issued `JobFacts` publication claim.
pub(crate) struct JobFactsReadContext<'a> {
    pub(crate) reservation_id: i64,
    pub(crate) operation_id: i64,
    pub(crate) connection_id: &'a str,
    pub(crate) engine_id: &'a str,
    pub(crate) docker_root_digest: &'a str,
    pub(crate) runner_container_id: &'a str,
    pub(crate) dind_container_id: &'a str,
    pub(crate) runner_id: i64,
    pub(crate) runner_name: &'a str,
    pub(crate) scale_set_id: i64,
    pub(crate) instance_binding_digest: &'a str,
    pub(crate) profile_generation_id: &'a str,
    pub(crate) profile_digest: &'a str,
    pub(crate) sample_generation_id: &'a str,
}

/// Read and validate the complete storage document inside one read transaction.
pub(crate) async fn read_job_facts_snapshot(
    journal: &ConnectionJournal,
    context: JobFactsReadContext<'_>,
    ready: &VerifiedStorageInitializationReady,
    channel: &VerifiedProofChannelBinding,
) -> Result<BoundJobFactsSnapshot, HostError> {
    validate_inputs(journal, &context, ready, channel)?;
    read_transaction(journal.journal_ref(), async |connection| {
        journal.validate_scope(connection).await?;
        read_snapshot(connection, journal, &context, ready, channel).await
    })
    .await
}

fn validate_inputs(
    journal: &ConnectionJournal,
    context: &JobFactsReadContext<'_>,
    ready: &VerifiedStorageInitializationReady,
    channel: &VerifiedProofChannelBinding,
) -> Result<(), HostError> {
    if context.reservation_id <= 0
        || context.operation_id <= 0
        || context.connection_id != journal.connection_id().as_str()
        || !channel.valid()
        || !channel.matches_initialized_job_facts(ready)
        || ready.reservation_id() != context.reservation_id
        || ready.operation_id() != context.operation_id
        || ready.connection_id() != context.connection_id
        || ready.engine_id() != context.engine_id
        || ready.docker_root_digest() != context.docker_root_digest
        || channel.runner_id() != context.runner_id
        || channel.runner_name() != context.runner_name
        || channel.scale_set_id() != context.scale_set_id
        || channel.runner_container_id() != context.runner_container_id
        || channel.dind_container_id() != context.dind_container_id
        || channel.instance_binding_digest() != context.instance_binding_digest
        || !values::valid_digest(context.instance_binding_digest)
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn read_snapshot(
    connection: &turso::Connection,
    journal: &ConnectionJournal,
    context: &JobFactsReadContext<'_>,
    ready: &VerifiedStorageInitializationReady,
    channel: &VerifiedProofChannelBinding,
) -> Result<BoundJobFactsSnapshot, HostError> {
    let facts = state::read_launch_facts(connection, journal, context).await?;
    let profile = super::super::profile_generation(connection, &facts.profile_generation)
        .await?
        .ok_or(HostError::Journal)?;
    if profile.profile_digest() != facts.profile_digest {
        return Err(HostError::Identity);
    }
    let storage = roles::read_storage_roles(connection, context, &profile).await?;
    let cgroups = projection::read_cgroups(connection, context).await?;
    projection::build_snapshot(context, facts, storage, cgroups, ready, channel)
}
