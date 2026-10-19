use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::connection::storage::read_transaction;
use crate::journal::scale_set_create::{CreatedScaleSetResponse, ScaleSetCreateRequest};
use crate::journal::{Journal, SourceAuthorizationResult, fixture_provisioning_profile};
use crate::launch::steps::source::{AuthorizedSourceReceipt, test_receipt};
use velnor_runner_github::SourceMetadata;
use velnor_runner_github::workflow_run::{
    VerifiedJobWorkflowRef, VerifiedWorkflowSource, WorkflowRefKind,
};

macro_rules! trace_test_stage {
    ($_label:literal, $stage:expr) => {
        $stage.await?
    };
}

pub(crate) const TEST_ENGINE_ID: &str = "11111111-1111-4111-8111-111111111111";

pub(crate) fn test_initializer_limits()
-> Result<crate::worker::storage_initialization::StorageInitializationLimits, HostError> {
    crate::worker::storage_initialization::StorageInitializationLimits::from_reserved_budget(
        crate::worker::test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )
}

pub(crate) async fn storage_initializer_abort_state(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<String, HostError> {
    read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection
            .query(
                "SELECT abort_state FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        row.get::<String>(0).map_err(|_| HostError::Journal)
    })
    .await
}

pub(crate) async fn bootstrap_start_state(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<String, HostError> {
    read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection
            .query(
                "SELECT bootstrap_start_state FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        row.get::<String>(0).map_err(|_| HostError::Journal)
    })
    .await
}

pub(crate) struct StorageInitializerPhase {
    pub(crate) start: String,
    pub(crate) result: String,
    pub(crate) remove: String,
    pub(crate) abort: String,
}

pub(crate) async fn storage_initializer_phase(
    journal: &Journal,
    reservation_id: i64,
) -> Result<StorageInitializerPhase, HostError> {
    read_transaction(journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT start_state, result_state, remove_state, abort_state FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let phase = StorageInitializerPhase {
            start: row.get::<String>(0).map_err(|_| HostError::Journal)?,
            result: row.get::<String>(1).map_err(|_| HostError::Journal)?,
            remove: row.get::<String>(2).map_err(|_| HostError::Journal)?,
            abort: row.get::<String>(3).map_err(|_| HostError::Journal)?,
        };
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        Ok(phase)
    })
    .await
}

pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-request-journal-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    pub(crate) fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct ListenerDeliveryFixture {
    pub(crate) _scratch: Scratch,
    pub(crate) scope: ConnectionJournal,
    pub(crate) claim: crate::journal::ListenerDeliveryClaim,
    pub(crate) runner: velnor_runner_github::RunnerReference,
}

pub(crate) async fn listener_delivery_fixture(
    nonce: char,
) -> Result<ListenerDeliveryFixture, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(super::admission::JOB_LIMIT)
        .await?;
    let repository = binding("listener-fixture", 101)?;
    let scope = created_scope(&journal, &repository, 501).await?;
    let initialized =
        provision_acquired_request_with_initialization(&scope, 70, nonce, nonce).await?;
    let runner = velnor_runner_github::RunnerReference {
        id: 770,
        name: runner_name(nonce),
        runner_scale_set_id: 501,
    };
    let claim =
        prepare_initialized_listener_delivery(&scope, &repository, &initialized, &runner, 70)
            .await?;
    Ok(ListenerDeliveryFixture {
        _scratch: scratch,
        scope,
        claim,
        runner,
    })
}

pub(crate) fn binding(id: &str, repository_id: i64) -> Result<RepositoryConnection, HostError> {
    let labels = vec!["velnor".to_owned(), "integration".to_owned()];
    RepositoryConnection::try_new(
        ConnectionId::parse(id)?,
        repository_id,
        format!("owner-{repository_id}/repo-{repository_id}"),
        KeychainReference::parse(&format!("keychain:velnor/{id}"))?,
        ScaleSetSelector::try_new("integration", &labels)?,
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )?,
    )
}

pub(crate) async fn created_scope(
    journal: &Journal,
    binding: &RepositoryConnection,
    set_id: i64,
) -> Result<ConnectionJournal, HostError> {
    journal
        .configure_storage_profile(fixture_provisioning_profile())
        .await?;
    journal.bind_engine(TEST_ENGINE_ID).await?;
    let scope = journal.connection_scope(binding).await?;
    let request = ScaleSetCreateRequest::new(binding, 7)?;
    let prepared = scope.prepare_scale_set_create(&request).await?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await?
        .ok_or(HostError::Journal)?;
    let response = CreatedScaleSetResponse::verified(
        &request,
        set_id,
        "integration",
        binding.scale_set.labels(),
        true,
    )?;
    scope.record_scale_set_created(claim, response).await?;
    scope
        .bind_engine_context(TEST_ENGINE_ID, "unix:///var/run/docker.sock", "linux/amd64")
        .await?;
    let config = crate::config::HostConfig::test_with_job_authority_policy(binding)?;
    scope.install_job_authority_policy(&config).await?;
    Ok(scope)
}

pub(super) async fn receipt(
    scope: &ConnectionJournal,
    request_id: i64,
    message_id: i64,
    repository_id: i64,
) -> Result<AuthorizedSourceReceipt, HostError> {
    let binding = scope.source_authorization_binding().await?;
    Ok(test_receipt(
        &binding,
        request_id,
        message_id,
        source(repository_id, request_id + 1000),
        SourcePolicyMode::Repository,
    ))
}

pub(super) fn source(repository_id: i64, run_id: i64) -> VerifiedWorkflowSource {
    let workflow_ref = format!(
        "owner-{repository_id}/repo-{repository_id}/.github/workflows/build.yml@refs/heads/main"
    );
    VerifiedWorkflowSource {
        repository_id,
        owner_name: format!("owner-{repository_id}"),
        repository_name: format!("repo-{repository_id}"),
        workflow_run_id: run_id,
        event_name: "push".to_owned(),
        head_branch: Some("main".to_owned()),
        head_sha: "a".repeat(40),
        job_workflow_ref: workflow_ref.clone(),
        job_workflow: VerifiedJobWorkflowRef {
            full_ref: workflow_ref,
            owner_name: format!("owner-{repository_id}"),
            repository_name: format!("repo-{repository_id}"),
            workflow_path: ".github/workflows/build.yml".to_owned(),
            reference: "refs/heads/main".to_owned(),
            kind: WorkflowRefKind::Branch,
        },
        workflow_source_sha: None,
        run_attempt: 1,
    }
}

pub(super) fn event_source(repository_id: i64, run_id: i64) -> SourceMetadata {
    SourceMetadata {
        owner_name: Some(format!("owner-{repository_id}")),
        repository_name: Some(format!("repo-{repository_id}")),
        job_workflow_ref: Some(format!(
            "owner-{repository_id}/repo-{repository_id}/.github/workflows/build.yml@refs/heads/main"
        )),
        workflow_run_id: Some(run_id),
        event_name: Some("push".to_owned()),
    }
}

pub(super) fn runner_name(nonce: char) -> String {
    format!("v{}", nonce.to_string().repeat(32))
}

pub(crate) async fn record_acquired(
    scope: &ConnectionJournal,
    request_id: i64,
) -> Result<(), HostError> {
    let identity = scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id })
        .await?;
    let source = receipt(scope, request_id, request_id + 100, 101).await?;
    if scope
        .record_source_authorization(&identity, &source)
        .await?
        != SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }
    if !scope.claim_request_acquire(&identity).await? {
        return Err(HostError::Journal);
    }
    scope
        .record_request_acquire(
            &identity,
            crate::journal::AssignedAcquireResult::Returned(vec![request_id]),
        )
        .await
}

include!("helpers/physical.rs");
include!("helpers/cleanup.rs");
include!("helpers/delivery.rs");

pub(crate) async fn provision_physical_intent_with_initialization(
    scope: &ConnectionJournal,
    nonce: char,
    sample_nonce: char,
) -> Result<JobFactsReaderFixture, HostError> {
    use crate::journal::AssignmentKey;
    let nonce_text = nonce.to_string().repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce_text.clone(),
        })
        .await?;
    provision_assignment_with_initialization(scope, assignment, None, nonce, sample_nonce).await
}

pub(crate) async fn provision_acquired_request_with_initialization(
    scope: &ConnectionJournal,
    request_id: i64,
    nonce: char,
    sample_nonce: char,
) -> Result<JobFactsReaderFixture, HostError> {
    provision_acquired_request_with_initialization_for_repository(
        scope,
        request_id,
        nonce,
        sample_nonce,
        101,
    )
    .await
}

pub(crate) async fn provision_acquired_request_with_initialization_for_repository(
    scope: &ConnectionJournal,
    request_id: i64,
    nonce: char,
    sample_nonce: char,
    repository_id: i64,
) -> Result<JobFactsReaderFixture, HostError> {
    use crate::journal::{AssignmentKey, SourceAuthorizationResult};
    let request = scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id })
        .await?;
    let source = receipt(scope, request_id, request_id + 100, repository_id).await?;
    if scope.record_source_authorization(&request, &source).await?
        != SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    provision_assignment_with_initialization(scope, assignment, Some(request), nonce, sample_nonce)
        .await
}

async fn provision_assignment_with_initialization(
    scope: &ConnectionJournal,
    assignment: crate::journal::AssignmentIdentity,
    request: Option<crate::journal::AssignmentIdentity>,
    nonce: char,
    sample_nonce: char,
) -> Result<JobFactsReaderFixture, HostError> {
    use crate::journal::{
        ReservationRequest, ReservationResult, fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    const JOB_MEMORY: u64 = 8_589_934_592;
    let sample_generation = sample_generation_for_test(sample_nonce);
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let physical = ReservationRequest {
        assignment: &assignment,
        engine_id: TEST_ENGINE_ID,
        docker_root_digest: &docker_root_digest,
        cpu_millicores: 2_000,
        memory_bytes: JOB_MEMORY,
        sample_generation_id: &sample_generation_id,
    };
    let reservation_id = if let Some(request) = request.as_ref() {
        let crate::journal::RequestPhysicalReservationResult::AcquireClaimed {
            reservation_id, ..
        } = scope
            .try_reserve_and_claim_request_acquire(physical, permit, admission, request)
            .await?
        else {
            return Err(HostError::Journal);
        };
        scope
            .record_request_acquire(
                request,
                crate::journal::AssignedAcquireResult::Returned(vec![
                    request.request_id().ok_or(HostError::Identity)?,
                ]),
            )
            .await?;
        reservation_id
    } else {
        let ReservationResult::Reserved { reservation_id } = scope
            .try_reserve_with_provisioning(physical, permit, admission)
            .await?
        else {
            return Err(HostError::Journal);
        };
        reservation_id
    };
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh {
        return Err(HostError::Journal);
    }
    let artifacts = Box::pin(provisioning::provision_with_initialization(
        scope,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    ))
    .await?;
    let provisioned = artifacts.provisioned;
    Ok(JobFactsReaderFixture {
        // Physical reservation stays bound to the Remote assignment, while
        // acquired request events use the exact AssignedRequest identity.
        assignment: request.unwrap_or_else(|| provisioned.assignment.clone()),
        reservation_assignment: provisioned.assignment.clone(),
        provisioned: provisioned.clone(),
        reservation_id: provisioned.reservation_id,
        intent_id: provisioned.intent_id,
        runner_container_id: provisioned.runner_container_id,
        dind_container_id: provisioned.dind_container_id,
        docker_root_digest: provisioned.docker_root_digest,
        sample_generation_id: provisioned.sample_generation_id,
        instance_binding_digest: provisioned.instance_binding_digest,
        claim: artifacts.claim,
        handoff: artifacts.handoff,
        observed: artifacts.observed,
        ready: artifacts.ready,
    })
}

pub(crate) struct JobFactsReaderFixture {
    pub(crate) assignment: crate::journal::AssignmentIdentity,
    pub(crate) reservation_assignment: crate::journal::AssignmentIdentity,
    pub(super) provisioned: ProvisionedRunner,
    pub(crate) reservation_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) runner_container_id: String,
    pub(crate) dind_container_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) sample_generation_id: String,
    pub(crate) instance_binding_digest: String,
    pub(crate) claim: crate::journal::StorageInitializationClaim,
    pub(crate) handoff: crate::journal::VerifiedMetadataInitializationHandoff,
    pub(crate) observed: crate::worker::storage_initialization::ObservedStorageInitialization,
    pub(crate) ready: crate::worker::storage_initialization::VerifiedStorageInitializationReady,
}

pub(crate) async fn initialize_reserved_request(
    scope: &ConnectionJournal,
    request: crate::journal::AssignmentIdentity,
    physical: crate::journal::AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<JobFactsReaderFixture, HostError> {
    let artifacts = Box::pin(provisioning::provision_with_initialization(
        scope,
        physical.clone(),
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    ))
    .await?;
    let provisioned = artifacts.provisioned;
    Ok(JobFactsReaderFixture {
        assignment: request,
        reservation_assignment: physical,
        reservation_id: provisioned.reservation_id,
        intent_id: provisioned.intent_id,
        runner_container_id: provisioned.runner_container_id.clone(),
        dind_container_id: provisioned.dind_container_id.clone(),
        docker_root_digest: provisioned.docker_root_digest.clone(),
        sample_generation_id: provisioned.sample_generation_id.clone(),
        instance_binding_digest: provisioned.instance_binding_digest.clone(),
        provisioned,
        claim: artifacts.claim,
        handoff: artifacts.handoff,
        observed: artifacts.observed,
        ready: artifacts.ready,
    })
}

pub(crate) async fn provision_acquired_physical_intent(
    scope: &ConnectionJournal,
    engine_id: &str,
    assignment: crate::journal::AssignmentIdentity,
    nonce: &str,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<(), HostError> {
    provisioning::provision_with_nonce(
        scope,
        engine_id,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
    .map(|_| ())
}

pub(crate) async fn prepare_bootstrap_start_claim(
    scope: &ConnectionJournal,
    engine_id: &str,
    assignment: crate::journal::AssignmentIdentity,
    nonce: &str,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<
    (
        crate::journal::BootstrapStartClaim,
        crate::journal::VerifiedMetadataInitializationHandoff,
    ),
    HostError,
> {
    provisioning::prepare_bootstrap_start_claim(
        scope,
        engine_id,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
}
