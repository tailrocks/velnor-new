//! Build one actual job-facts publisher row for the v30 migration test.

use std::path::Path;

use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers;
use crate::journal::{
    CapacityLimits, ExecutionPublicationParent, ExecutionPublicationSeed, Journal,
};
use velnor_runner_github::RunnerReference;

pub(super) async fn create(path: &Path) -> String {
    create_result(path)
        .await
        .expect("create publication fixture")
}

async fn create_result(path: &Path) -> Result<String, HostError> {
    let journal = Journal::open(path).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 25_769_803_776,
            },
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: crate::worker::PROBE_CPU_MILLICORES,
                memory_bytes: crate::worker::PROBE_MEMORY_LIMIT_BYTES,
            },
        )
        .await?;
    let repository = helpers::binding("essential", 101)?;
    let scope = helpers::created_scope(&journal, &repository, 501).await?;
    let initialized =
        helpers::provision_acquired_request_with_initialization(&scope, 70, '8', '8').await?;
    let runner = RunnerReference {
        id: 770,
        name: format!("v{}", "8".repeat(32)),
        runner_scale_set_id: 501,
    };
    let (generation, digest) = scope.test_active_authority_identity().await?;
    let jit = scope
        .claim_runner_jit_for_request(
            initialized.intent_id,
            initialized.reservation_id,
            70,
            &runner.name,
            generation,
            &digest,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope.bind_jit_runner(jit, &runner).await?;
    let claim = scope
        .claim_job_facts_publication(initialized.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let metadata = initialized.ready.final_readiness();
    let boot_id = guest_boot_id(&journal, initialized.reservation_id).await?;
    let operation_id = "fedcba9876543210fedcba9876543210".to_owned();
    let parent =
        ExecutionPublicationParent::job_facts(&claim, metadata, &boot_id, &"f".repeat(64))?;
    let context = ExecutionPublicationSeed {
        operation_id: operation_id.clone(),
        instance_id: journal.instance_id().await?,
        engine_id: helpers::TEST_ENGINE_ID.to_owned(),
        docker_root_digest: initialized.docker_root_digest.clone(),
        source_revision: "c".repeat(40),
        runtime_image_id: format!("sha256:{}", "d".repeat(64)),
        image_binding_digest: "e".repeat(64),
        projection_digest: "a".repeat(64),
        recovery_projection_json: r#"{"schema_version":1}"#.to_owned(),
        parent,
    }
    .into_context(metadata)?;
    journal.prepare_execution_publication(context).await?;
    Ok(operation_id)
}

async fn guest_boot_id(journal: &Journal, reservation_id: i64) -> Result<String, HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT guest_boot_id FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    Ok(value)
}
