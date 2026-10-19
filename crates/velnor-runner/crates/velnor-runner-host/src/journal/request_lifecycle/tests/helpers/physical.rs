#[derive(Clone)]
pub(super) struct ProvisionedRunner {
    pub(super) assignment: crate::journal::AssignmentIdentity,
    pub(super) intent_id: i64,
    pub(super) reservation_id: i64,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) docker_root_digest: String,
    pub(super) sample_generation_id: String,
    pub(super) instance_binding_digest: String,
    pub(super) storage_plan: crate::journal::StorageProvisioningPlan,
    pub(super) nonce: String,
}

pub(super) async fn provision_physical_intent(
    scope: &ConnectionJournal,
    nonce: char,
) -> Result<ProvisionedRunner, HostError> {
    provision_physical_intent_with_sample(scope, nonce, nonce).await
}

pub(super) async fn provision_physical_intent_with_sample(
    scope: &ConnectionJournal,
    nonce: char,
    sample_nonce: char,
) -> Result<ProvisionedRunner, HostError> {
    use crate::journal::{
        AssignmentKey, ReservationRequest, ReservationResult,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    const JOB_MEMORY: u64 = 8_589_934_592;
    let nonce_text = nonce.to_string().repeat(32);
    let sample_generation = sample_nonce.to_string().repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce_text,
        })
        .await?;
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await?;
    let ReservationResult::Reserved { reservation_id } = result else {
        return Err(HostError::Journal);
    };
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh {
        return Err(HostError::Journal);
    }

    provision_reserved_physical_intent(
        scope,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
}

pub(super) async fn provision_reserved_physical_intent(
    scope: &ConnectionJournal,
    assignment: crate::journal::AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<ProvisionedRunner, HostError> {
    provisioning::provision(
        scope,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
}

#[path = "physical/provisioning.rs"]
mod provisioning;

fn role_filesystem_digest(role: crate::journal::StorageRole) -> String {
    use crate::journal::StorageRole;
    match role {
        StorageRole::RunnerRoot | StorageRole::DindRoot | StorageRole::EngineMetadata => {
            "a".repeat(64)
        }
        StorageRole::RunnerHome => "2".repeat(64),
        StorageRole::RunnerWork => "3".repeat(64),
        StorageRole::RunnerTmp => "4".repeat(64),
        StorageRole::RunnerShm => "5".repeat(64),
        StorageRole::SharedRun => "6".repeat(64),
        StorageRole::DindData => "7".repeat(64),
        StorageRole::DindTmp => "8".repeat(64),
        StorageRole::DindShm => "9".repeat(64),
        StorageRole::CacheLive | StorageRole::CacheStage => "a".repeat(64),
    }
}
