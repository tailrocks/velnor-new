use super::fixture::{BoundFixture, bound_fixture};
use crate::error::HostError;
use crate::journal::request_lifecycle::tests::helpers::{binding, created_scope};

#[tokio::test]
async fn bound_journal_snapshot_rejects_foreign_scope_and_missing_role() {
    let fixture = bound_fixture('c').await.expect("bound fixture");
    let foreign_repository = binding("foreign-job-facts-reader", 102).expect("binding");
    let foreign_scope = created_scope(&fixture.journal, &foreign_repository, 502)
        .await
        .expect("foreign scope");
    assert!(matches!(
        foreign_scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
            .await,
        Err(HostError::Identity)
    ));

    let guarded_delete = crate::journal::connection::storage::in_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            connection
                .execute(
                    "DELETE FROM capacity_reservation_bound_storage_roles WHERE reservation_id = ?1 AND role = 'runner-tmp'",
                    [fixture.claim.reservation_id()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        },
    )
    .await;
    assert!(
        guarded_delete.is_err(),
        "production bound-role guard remains active"
    );

    crate::journal::connection::storage::in_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            // The production journal deliberately makes bound role rows
            // immutable. Drop that guard only in this scratch database to
            // model persisted corruption and exercise the reader's fail-closed
            // behavior when a required role is absent.
            connection
                .execute("DROP TRIGGER bound_storage_no_delete", ())
                .await
                .map_err(|_| HostError::Journal)?;
            connection
                .execute(
                    "DELETE FROM capacity_reservation_bound_storage_roles WHERE reservation_id = ?1 AND role = 'runner-tmp'",
                    [fixture.claim.reservation_id()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        },
    )
    .await
    .expect("remove one role from test journal");
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn bound_journal_snapshot_rejects_foreign_ready_and_channel() {
    let fixture = bound_fixture('d').await.expect("bound fixture");
    let other = Box::pin(bound_fixture('e')).await.expect("other fixture");
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &other.ready.ready, &fixture.channel)
            .await
            .is_err()
    );
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &other.channel)
            .await
            .is_err()
    );
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&other.claim, &fixture.ready.ready, &fixture.channel)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn bound_journal_snapshot_rejects_changed_profile_sample_and_pair() {
    let fixture = bound_fixture('f').await.expect("bound fixture");
    let reservation_id = fixture.claim.reservation_id();
    assert_text_witness_immutable(
        &fixture,
        "UPDATE capacity_reservation_provisioning SET profile_digest = ?1 WHERE reservation_id = ?2",
        "f".repeat(64),
        reservation_id,
    )
    .await;
    assert_text_witness_immutable(
        &fixture,
        "UPDATE capacity_reservation_provisioning SET sample_generation_id = ?1 WHERE reservation_id = ?2",
        "e".repeat(32),
        reservation_id,
    )
    .await;
    let pair_mutation = crate::journal::connection::storage::in_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET runner_container_id = ?1 WHERE reservation_id = ?2",
                    ("foreign-runner-container", reservation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        },
    )
    .await;
    assert!(pair_mutation.is_err(), "the durable pair is immutable");
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn bound_journal_snapshot_rejects_changed_role_charge() {
    let fixture = bound_fixture('g').await.expect("bound fixture");
    let changed_charge = crate::journal::connection::storage::in_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            connection
                .execute(
                    "UPDATE capacity_reservation_bound_storage_roles SET charge_bytes = charge_bytes + 1 WHERE reservation_id = ?1 AND role = 'runner-work'",
                    [fixture.claim.reservation_id()],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        },
    )
    .await;
    assert!(changed_charge.is_err(), "bound charges are immutable");
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn bound_journal_snapshot_rejects_changed_jit_and_publication_phase() {
    let fixture = bound_fixture('h').await.expect("bound fixture");
    let reservation_id = fixture.claim.reservation_id();
    let operation_id = fixture.claim.operation_id();
    update_jit_runner(&fixture, 771, operation_id).await;
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
            .await
            .is_err()
    );
    update_jit_runner(&fixture, fixture.claim.runner().id, operation_id).await;
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
            .await
            .is_ok()
    );
    crate::journal::connection::storage::in_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            connection
                .execute(
                    "UPDATE capacity_reservation_provisioning_state SET publication_state = 'uncertain' WHERE reservation_id = ?1",
                    [reservation_id],
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        },
    )
    .await
    .expect("mark publication uncertain in scratch journal");
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
            .await
            .is_err()
    );
}

async fn update_jit_runner(fixture: &BoundFixture, runner_id: i64, operation_id: i64) {
    crate::journal::connection::storage::in_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            connection
                .execute(
                    "UPDATE connection_runner_jits SET runner_id = ?1 WHERE intent_id = ?2",
                    (runner_id, operation_id),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        },
    )
    .await
    .expect("change JIT identity in scratch journal");
}

async fn assert_text_witness_immutable(
    fixture: &BoundFixture,
    sql: &str,
    changed: String,
    reservation_id: i64,
) {
    let result = crate::journal::connection::storage::in_transaction(
        fixture.scope.journal_ref(),
        async |connection| {
            connection
                .execute(sql, (changed.as_str(), reservation_id))
                .await
                .map_err(|_| HostError::Journal)?;
            Ok(())
        },
    )
    .await;
    assert!(result.is_err(), "the durable witness is immutable");
    assert!(
        fixture
            .scope
            .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
            .await
            .is_ok()
    );
}
