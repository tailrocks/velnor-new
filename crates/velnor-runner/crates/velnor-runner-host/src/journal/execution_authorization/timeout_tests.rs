use super::{StartedRequestTimeoutResult, TimeoutState, load_timeout_state};
use crate::error::HostError;
use crate::worker::execution_authorization::{
    VerifiedProofChannelBinding, test_proof_channel_mount, test_verified_proof_channel_binding,
};

#[tokio::test]
async fn timeout_api_denies_exact_started_request_and_keeps_charge() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    fixture.record_started().await?;
    let NativeStartedParts {
        scratch,
        journal,
        scope,
        request_id,
        channel,
        ..
    } = NativeStartedParts::from(fixture);

    assert_eq!(
        scope
            .deny_started_request_timeout(&channel, request_id)
            .await?,
        StartedRequestTimeoutResult::Enqueued
    );
    assert_eq!(
        scope
            .deny_started_request_timeout(&channel, request_id)
            .await?,
        StartedRequestTimeoutResult::AlreadyEnqueued
    );
    assert_eq!(
        scope
            .deny_started_request_timeout(&channel, request_id + 1)
            .await?,
        StartedRequestTimeoutResult::NotEligible
    );

    let connection = journal.connection().await?;
    assert_timeout_state(&connection, request_id, true).await?;
    drop(connection);
    drop(scope);
    drop(journal);

    // Opening a journal after timeout must preserve the denied proof and its
    // cleanup charge. It must not turn the timeout into completion or release.
    let reopened = crate::journal::Journal::open(&scratch.file()).await?;
    let connection = reopened.connection().await?;
    assert_timeout_state(&connection, request_id, true).await?;
    drop(connection);
    Ok(())
}

#[tokio::test]
async fn timeout_api_loses_to_a_real_completed_event() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    fixture.record_started().await?;
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _journal: journal,
        scope,
        request_id,
        message_id,
        assignment,
        runner,
        source,
        channel,
        ..
    } = fixture;
    scope
        .record_job_completed(
            &assignment,
            message_id + 1,
            &source,
            Some("success"),
            Some(&runner),
            Some("opaque-started-job"),
        )
        .await?;

    assert_eq!(
        scope
            .deny_started_request_timeout(&channel, request_id)
            .await?,
        StartedRequestTimeoutResult::NotEligible
    );
    let connection = journal.connection().await?;
    assert_completed_state(&connection, request_id).await?;
    Ok(())
}

struct NativeStartedParts {
    scratch: crate::journal::request_lifecycle::tests::helpers::Scratch,
    journal: crate::journal::Journal,
    scope: crate::journal::ConnectionJournal,
    request_id: i64,
    channel: VerifiedProofChannelBinding,
}

impl From<crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture>
    for NativeStartedParts
{
    fn from(fixture: crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture) -> Self {
        let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
            _scratch: scratch,
            _journal: journal,
            scope,
            request_id,
            channel,
            ..
        } = fixture;
        Self {
            scratch,
            journal,
            scope,
            request_id,
            channel,
        }
    }
}

async fn assert_timeout_state(
    connection: &turso::Connection,
    request_id: i64,
    expect_cleanup: bool,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT proof.challenge_state, proof.proof_state, proof.deny_code, proof.source_digest, proof.source_json, proof.actual_request_id, proof.started_event_digest, reservation.state, reservation.remote_cleanup_proven, reservation.local_cleanup_proven, intent.runner_completed, auth.completed_event_digest, jit.completed_request_id, (SELECT COUNT(*) FROM connection_completion_cleanup AS cleanup WHERE cleanup.intent_id = proof.intent_id AND cleanup.connection_id = proof.connection_id AND cleanup.scale_set_id = proof.scale_set_id AND cleanup.runner_request_id = ?1 AND cleanup.runner_id = proof.runner_id AND cleanup.runner_name = proof.runner_name), (SELECT COUNT(*) FROM connection_execution_decisions AS decision WHERE decision.intent_id = proof.intent_id AND decision.reservation_id = proof.reservation_id AND decision.connection_id = proof.connection_id) FROM connection_execution_proofs AS proof JOIN capacity_reservations AS reservation ON reservation.id = proof.reservation_id AND reservation.intent_id = proof.intent_id AND reservation.connection_id = proof.connection_id JOIN connection_intents AS intent ON intent.id = proof.intent_id AND intent.connection_id = proof.connection_id JOIN connection_runner_jits AS jit ON jit.intent_id = proof.intent_id AND jit.connection_id = proof.connection_id JOIN connection_request_authorizations AS auth ON auth.connection_id = proof.connection_id AND auth.scale_set_id = proof.scale_set_id AND auth.request_id = ?1 WHERE proof.witness_request_id = ?1",
            [request_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        if expect_cleanup {
            "abandoned"
        } else {
            "published"
        }
    );
    assert_eq!(
        row.get::<String>(1).map_err(|_| HostError::Journal)?,
        if expect_cleanup { "denied" } else { "pending" }
    );
    assert_eq!(
        row.get::<Option<String>>(2)
            .map_err(|_| HostError::Journal)?
            .as_deref(),
        expect_cleanup.then_some("proof_malformed")
    );
    assert!(
        row.get::<Option<String>>(3)
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    assert!(
        row.get::<Option<String>>(4)
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    assert!(
        row.get::<Option<i64>>(5)
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    assert!(
        row.get::<Option<String>>(6)
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    assert_eq!(
        row.get::<String>(7).map_err(|_| HostError::Journal)?,
        "reserved"
    );
    assert_eq!(row.get::<i64>(8).map_err(|_| HostError::Journal)?, 0);
    assert_eq!(row.get::<i64>(9).map_err(|_| HostError::Journal)?, 0);
    assert_eq!(row.get::<i64>(10).map_err(|_| HostError::Journal)?, 0);
    assert!(
        row.get::<Option<String>>(11)
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    assert!(
        row.get::<Option<i64>>(12)
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    assert_eq!(
        row.get::<i64>(13).map_err(|_| HostError::Journal)?,
        i64::from(expect_cleanup)
    );
    assert_eq!(row.get::<i64>(14).map_err(|_| HostError::Journal)?, 0);
    Ok(())
}

async fn assert_completed_state(
    connection: &turso::Connection,
    request_id: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT proof.challenge_state, proof.proof_state, proof.deny_code, auth.completed_event_digest, jit.completed_request_id, (SELECT COUNT(*) FROM connection_completion_cleanup AS cleanup WHERE cleanup.runner_request_id = ?1) FROM connection_execution_proofs AS proof JOIN connection_request_authorizations AS auth ON auth.connection_id = proof.connection_id AND auth.scale_set_id = proof.scale_set_id AND auth.request_id = ?1 JOIN connection_runner_jits AS jit ON jit.intent_id = proof.intent_id AND jit.connection_id = proof.connection_id WHERE proof.witness_request_id = ?1",
            [request_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        row.get::<String>(0).map_err(|_| HostError::Journal)?,
        "published"
    );
    assert_eq!(
        row.get::<String>(1).map_err(|_| HostError::Journal)?,
        "pending"
    );
    assert!(
        row.get::<Option<String>>(2)
            .map_err(|_| HostError::Journal)?
            .is_none()
    );
    assert!(
        row.get::<Option<String>>(3)
            .map_err(|_| HostError::Journal)?
            .is_some()
    );
    assert_eq!(
        row.get::<Option<i64>>(4).map_err(|_| HostError::Journal)?,
        Some(request_id)
    );
    assert_eq!(row.get::<i64>(5).map_err(|_| HostError::Journal)?, 1);
    Ok(())
}

#[tokio::test]
async fn timeout_classifier_requires_exact_live_started_pending_proof() -> Result<(), HostError> {
    let path = std::env::temp_dir().join(format!(
        "velnor-timeout-classifier-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_nanos()
    ));
    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Journal)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    create_schema(&connection).await?;
    let channel = test_channel()?;
    let channel_json =
        String::from_utf8(channel.canonical_json()?).map_err(|_| HostError::Journal)?;
    let channel_digest = channel.computed_digest()?;
    insert_live_rows(&connection, &channel, &channel_json, &channel_digest).await?;

    assert!(matches!(
        load_timeout_state(&connection, &channel, 100, &channel_json, &channel_digest,).await?,
        TimeoutState::Pending
    ));
    assert!(matches!(
        load_timeout_state(&connection, &channel, 101, &channel_json, &channel_digest,).await?,
        TimeoutState::NotEligible
    ));
    connection
        .execute(
            "UPDATE connection_request_authorizations SET completed_event_digest='completed' WHERE request_id=100",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(matches!(
        load_timeout_state(&connection, &channel, 100, &channel_json, &channel_digest,).await?,
        TimeoutState::NotEligible
    ));
    connection
        .execute(
            "UPDATE connection_request_authorizations SET completed_event_digest=NULL WHERE request_id=100",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    connection
        .execute(
            "UPDATE connection_execution_proofs SET challenge_state='abandoned', proof_state='denied', deny_code='proof_malformed' WHERE intent_id=12",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_completion_cleanup VALUES (12,'connection-1',13,100,701,'runner-701')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(matches!(
        load_timeout_state(&connection, &channel, 100, &channel_json, &channel_digest,).await?,
        TimeoutState::AlreadyEnqueued
    ));

    connection
        .execute(
            "UPDATE connection_execution_proofs SET deny_code='interrupted_after_restart' WHERE intent_id=12",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(matches!(
        load_timeout_state(&connection, &channel, 100, &channel_json, &channel_digest,).await?,
        TimeoutState::NotEligible
    ));
    drop(connection);
    drop(database);
    std::fs::remove_file(path).map_err(|_| HostError::Journal)?;
    Ok(())
}

fn test_channel() -> Result<VerifiedProofChannelBinding, HostError> {
    let control = test_proof_channel_mount(
        "/opt/velnor/source-proof/control".to_owned(),
        "source-proof/control".to_owned(),
        1,
        (1, 2),
        (1, 2),
        123,
        1,
        "d".repeat(64),
        10,
        (0, 0),
        0o555,
        true,
        true,
    )?;
    let inbox = test_proof_channel_mount(
        "/opt/velnor/source-proof/inbox".to_owned(),
        "source-proof/inbox".to_owned(),
        2,
        (0, 43),
        (0, 43),
        0x0102_1994,
        2,
        "f".repeat(64),
        11,
        (1000, 1000),
        0o700,
        false,
        true,
    )?;
    test_verified_proof_channel_binding(
        11,
        12,
        "connection-1".to_owned(),
        13,
        100,
        "engine".to_owned(),
        "e".repeat(64),
        701,
        "runner-701".to_owned(),
        "a".repeat(12),
        "b".repeat(12),
        "c".repeat(64),
        "metadata".to_owned(),
        "c".repeat(12),
        control,
        inbox,
        true,
    )
}

async fn create_schema(connection: &turso::Connection) -> Result<(), HostError> {
    for sql in [
        "CREATE TABLE connection_execution_proofs (intent_id INTEGER, reservation_id INTEGER, connection_id TEXT, scale_set_id INTEGER, witness_request_id INTEGER, channel_json TEXT, channel_digest TEXT, challenge_state TEXT, proof_state TEXT, source_digest TEXT, source_json TEXT, actual_request_id INTEGER, started_event_digest TEXT, generation INTEGER, policy_digest TEXT, binding_digest TEXT, engine_id TEXT, docker_root_digest TEXT, runner_id INTEGER, runner_name TEXT, runner_container_id TEXT, dind_container_id TEXT, instance_binding_digest TEXT, deny_code TEXT)",
        "CREATE TABLE capacity_reservations (id INTEGER, intent_id INTEGER, connection_id TEXT, binding_digest TEXT, state TEXT, remote_cleanup_proven INTEGER, local_cleanup_proven INTEGER)",
        "CREATE TABLE connection_intents (id INTEGER, connection_id TEXT, cleanup_proven INTEGER, runner_completed INTEGER)",
        "CREATE TABLE connection_runner_jits (intent_id INTEGER, connection_id TEXT, started_request_id INTEGER, completed_request_id INTEGER, state TEXT)",
        "CREATE TABLE capacity_reservation_provisioning_state (reservation_id INTEGER, resources_bound INTEGER, bootstrap_start_state TEXT, jit_state TEXT, delivery_state TEXT, storage_cleanup_proven INTEGER)",
        "CREATE TABLE connection_request_authorizations (connection_id TEXT, scale_set_id INTEGER, request_id INTEGER, started_event_digest TEXT, completed_event_digest TEXT)",
        "CREATE TABLE connection_execution_decisions (intent_id INTEGER, reservation_id INTEGER, connection_id TEXT)",
        "CREATE TABLE connection_completion_cleanup (intent_id INTEGER, connection_id TEXT, scale_set_id INTEGER, runner_request_id INTEGER, runner_id INTEGER, runner_name TEXT)",
    ] {
        connection
            .execute(sql, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

async fn insert_live_rows(
    connection: &turso::Connection,
    channel: &VerifiedProofChannelBinding,
    channel_json: &str,
    channel_digest: &str,
) -> Result<(), HostError> {
    connection
        .execute(
            "INSERT INTO connection_execution_proofs VALUES (12,11,'connection-1',13,100,?1,?2,'published','pending',NULL,NULL,NULL,NULL,1,?3,?4,'engine',?5,701,'runner-701',?6,?7,?8,NULL)",
            (channel_json, channel_digest, "d".repeat(64), "b".repeat(64), "e".repeat(64), channel.runner_container_id(), channel.dind_container_id(), channel.instance_binding_digest()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO capacity_reservations VALUES (11,12,'connection-1',?1,'reserved',0,0)",
            ["b".repeat(64)],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_intents VALUES (12,'connection-1',0,0)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_runner_jits VALUES (12,'connection-1',100,NULL,'started')",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection.execute("INSERT INTO capacity_reservation_provisioning_state VALUES (11,1,'started','bound','delivered',0)", ()).await.map_err(|_| HostError::Journal)?;
    connection.execute("INSERT INTO connection_request_authorizations VALUES ('connection-1',13,100,'started-event',NULL)", ()).await.map_err(|_| HostError::Journal)?;
    Ok(())
}
