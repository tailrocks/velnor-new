//! Request and physical runner journal regressions.

mod acquire_settlement;
mod acquired_population_read;
mod acquired_recovery;
mod admission;
mod authorized_offers;
mod cleanup_identity;
mod cleanup_independence;
mod correlation;
mod disk_capacity;
mod event_source;
pub(crate) mod helpers;
mod initialization;
mod legacy_migration;
mod linked_request_reopen;
mod phase_recovery;
mod pre_jit_crossed_assignment;
mod profile_lifecycle;
mod request_link;
mod review_counterexamples;
mod review_migration;
mod scope_exclusivity;
mod storage_object_claim;
mod uncertain_effects;
mod v16_settlement_liveness;
mod v18_topology_migration;
mod v19_cleanup_proof;
mod worker_ready;

/// A source-authorized request with a durable acquired state and JIT-ready runner.
pub(crate) struct NativeStartedIntakeFixture {
    pub(crate) _scratch: helpers::Scratch,
    pub(crate) journal: crate::journal::Journal,
    pub(crate) database_path: std::path::PathBuf,
    pub(crate) scope: crate::journal::ConnectionJournal,
    pub(crate) request_id: i64,
    pub(crate) message_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) reservation_id: i64,
    pub(crate) assignment: crate::journal::AssignmentIdentity,
    pub(crate) reservation_assignment: crate::journal::AssignmentIdentity,
    pub(crate) runner: velnor_runner_github::RunnerReference,
    pub(crate) source: velnor_runner_github::SourceMetadata,
    pub(crate) channel: crate::worker::execution_authorization::VerifiedProofChannelBinding,
}

impl NativeStartedIntakeFixture {
    /// Record the fixture's exact Started event through the production journal API.
    pub(crate) async fn record_started(&self) -> Result<(), crate::error::HostError> {
        self.scope
            .record_job_started(
                &self.assignment,
                self.message_id,
                &self.source,
                &self.runner,
                Some("opaque-started-job"),
            )
            .await
            .map(|_| ())
    }
}

/// Test fixture for source checks.
pub(crate) struct NativePreChallengeFixture {
    pub(crate) _scratch: helpers::Scratch,
    pub(crate) _journal: crate::journal::Journal,
    pub(crate) database_path: std::path::PathBuf,
    pub(crate) scope: crate::journal::ConnectionJournal,
    pub(crate) initialized: helpers::JobFactsReaderFixture,
    pub(crate) runner: velnor_runner_github::RunnerReference,
    pub(crate) source: velnor_runner_github::SourceMetadata,
    pub(crate) channel: crate::worker::execution_authorization::VerifiedProofChannelBinding,
}

/// Prepare the test fixture before the source check.
pub(crate) async fn native_pre_challenge_fixture()
-> Result<NativePreChallengeFixture, crate::error::HostError> {
    use crate::journal::{CapacityLimits, Journal};

    const REQUEST_ID: i64 = 101;
    const SET_ID: i64 = 501;
    const REPOSITORY_ID: i64 = 101;
    const RUN_ID: i64 = REQUEST_ID + 1_000;

    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("native-started-intake", REPOSITORY_ID)?;
    let scope = helpers::created_scope(&journal, &binding, SET_ID).await?;
    let initialized =
        helpers::provision_acquired_request_with_initialization(&scope, REQUEST_ID, 'a', 'a')
            .await?;
    let provisioned = &initialized.provisioned;
    let runner = velnor_runner_github::RunnerReference {
        id: 701,
        name: helpers::runner_name('a'),
        runner_scale_set_id: SET_ID,
    };
    helpers::prepare_jit_and_job_facts(&scope, provisioned, &runner, REQUEST_ID).await?;
    let channel = crate::worker::execution_authorization::test_channel_for_ready_job_facts(
        crate::worker::execution_authorization::TestProofChannelIdentity {
            reservation_id: initialized.reservation_id,
            operation_id: initialized.intent_id,
            connection_id: scope.connection_id().as_str().to_owned(),
            scale_set_id: SET_ID,
            witness_request_id: REQUEST_ID,
            engine_id: helpers::TEST_ENGINE_ID.to_owned(),
            docker_root_digest: initialized.docker_root_digest.clone(),
            runner_id: runner.id,
            runner_name: runner.name.clone(),
            runner_container_id: initialized.runner_container_id.clone(),
            dind_container_id: initialized.dind_container_id.clone(),
            instance_binding_digest: initialized.instance_binding_digest.clone(),
        },
        &initialized.ready,
    )?;
    let config = crate::config::HostConfig::test_with_job_authority_policy(&binding)?;
    scope.install_job_authority_policy(&config).await?;
    let source = helpers::event_source(REPOSITORY_ID, RUN_ID);
    let database_path = scratch.file();

    Ok(NativePreChallengeFixture {
        _scratch: scratch,
        _journal: journal,
        database_path,
        scope,
        initialized,
        runner,
        source,
        channel,
    })
}

/// Publish the challenge for schema-migration fixtures that need a durable proof row.
pub(crate) async fn publish_native_fixture_challenge(
    fixture: &NativePreChallengeFixture,
) -> Result<(), crate::error::HostError> {
    helpers::publish_test_execution_challenge(&fixture.scope, &fixture.channel).await
}

/// Build the durable state required for a native Started message.
pub(crate) async fn native_started_intake_fixture()
-> Result<NativeStartedIntakeFixture, crate::error::HostError> {
    native_started_intake_fixture_with_filesystem_digest(None).await
}

pub(crate) async fn native_started_intake_fixture_with_filesystem_digest(
    filesystem_instance_digest: Option<String>,
) -> Result<NativeStartedIntakeFixture, crate::error::HostError> {
    use crate::journal::{CapacityLimits, Journal, OperationalCapacityLimits};

    const REQUEST_ID: i64 = 101;
    const MESSAGE_ID: i64 = 1_501;
    const SET_ID: i64 = 501;
    const REPOSITORY_ID: i64 = 101;
    const RUN_ID: i64 = REQUEST_ID + 1_000;

    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 25_769_803_776,
            },
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let binding = helpers::binding("native-started-intake", REPOSITORY_ID)?;
    let scope = helpers::created_scope(&journal, &binding, SET_ID).await?;
    let initialized = if let Some(digest) = filesystem_instance_digest.as_ref() {
        helpers::provision_acquired_request_with_initialization_and_filesystem_digest(
            &scope,
            REQUEST_ID,
            'a',
            'a',
            digest.clone(),
        )
        .await?
    } else {
        helpers::provision_acquired_request_with_initialization(&scope, REQUEST_ID, 'a', 'a')
            .await?
    };
    let provisioned = &initialized.provisioned;
    let runner = velnor_runner_github::RunnerReference {
        id: 701,
        name: helpers::runner_name('a'),
        runner_scale_set_id: SET_ID,
    };
    if let Some(digest) = filesystem_instance_digest.as_deref() {
        helpers::prepare_jit_and_job_facts_with_filesystem_digest(
            &scope,
            provisioned,
            &runner,
            REQUEST_ID,
            digest,
        )
        .await?;
    } else {
        helpers::prepare_jit_and_job_facts(&scope, provisioned, &runner, REQUEST_ID).await?;
    }
    let channel = crate::worker::execution_authorization::test_channel_for_ready_job_facts(
        crate::worker::execution_authorization::TestProofChannelIdentity {
            reservation_id: initialized.reservation_id,
            operation_id: initialized.intent_id,
            connection_id: scope.connection_id().as_str().to_owned(),
            scale_set_id: SET_ID,
            witness_request_id: REQUEST_ID,
            engine_id: helpers::TEST_ENGINE_ID.to_owned(),
            docker_root_digest: initialized.docker_root_digest.clone(),
            runner_id: runner.id,
            runner_name: runner.name.clone(),
            runner_container_id: initialized.runner_container_id.clone(),
            dind_container_id: initialized.dind_container_id.clone(),
            instance_binding_digest: initialized.instance_binding_digest.clone(),
        },
        &initialized.ready,
    )?;
    let config = crate::config::HostConfig::test_with_job_authority_policy(&binding)?;
    scope.install_job_authority_policy(&config).await?;
    helpers::publish_test_execution_challenge(&scope, &channel).await?;
    helpers::finish_listener_delivery_after_challenge(&scope, provisioned, &runner).await?;
    let source = helpers::event_source(REPOSITORY_ID, RUN_ID);
    let database_path = scratch.file();

    Ok(NativeStartedIntakeFixture {
        _scratch: scratch,
        journal,
        database_path,
        scope,
        request_id: REQUEST_ID,
        message_id: MESSAGE_ID,
        intent_id: initialized.intent_id,
        reservation_id: initialized.reservation_id,
        assignment: initialized.assignment,
        reservation_assignment: initialized.provisioned.assignment.clone(),
        runner,
        source,
        channel,
    })
}

#[tokio::test]
async fn published_challenge_without_source_survives_journal_reopen()
-> Result<(), crate::error::HostError> {
    let fixture = native_pre_challenge_fixture().await?;
    helpers::publish_test_execution_challenge(&fixture.scope, &fixture.channel).await?;
    let NativePreChallengeFixture {
        _scratch,
        _journal,
        database_path,
        scope,
        initialized,
        runner,
        source,
        channel,
    } = fixture;
    drop(scope);
    drop(_journal);
    drop(initialized);
    drop(channel);
    drop(runner);
    drop(source);
    let reopened = crate::journal::Journal::open(&database_path).await?;
    let mut rows = reopened
        .connection()
        .await?
        .query(
            "SELECT proof_state, challenge_state, actual_request_id, source_digest, source_json FROM connection_execution_proofs",
            (),
        )
        .await
        .map_err(|_| crate::error::HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| crate::error::HostError::Journal)?
        .ok_or(crate::error::HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0)
            .map_err(|_| crate::error::HostError::Journal)?,
        "pending"
    );
    assert_eq!(
        row.get::<String>(1)
            .map_err(|_| crate::error::HostError::Journal)?,
        "published"
    );
    assert!(
        row.get::<Option<i64>>(2)
            .map_err(|_| crate::error::HostError::Journal)?
            .is_none()
    );
    for column in [3, 4] {
        assert!(
            row.get::<Option<String>>(column)
                .map_err(|_| crate::error::HostError::Journal)?
                .is_none()
        );
    }
    Ok(())
}
