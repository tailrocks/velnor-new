//! Atomic reservation and fail-closed schema regressions.

use super::*;

#[tokio::test]
async fn permit_generation_is_one_use_and_fresh_resume_reuses_the_charge() -> Result<(), HostError>
{
    let scratch = Scratch::new("permit-resume")?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(limits(1, 2_000, 8_589_934_592))
        .await?;
    let scope = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let assignment = message_assignment(&scope, 17).await?;
    let initial_generation = "b".repeat(32);
    let initial_permit = permit_with_generation(&initial_generation);
    let initial_storage = crate::journal::fixture_admission(&initial_permit)?;
    let ReservationResult::Reserved { reservation_id } = scope
        .try_reserve_with_storage(
            request(&assignment, &initial_generation),
            initial_permit,
            initial_storage,
        )
        .await?
    else {
        return Err(HostError::Journal);
    };

    let replayed_permit = permit_with_generation(&initial_generation);
    let replayed_storage = crate::journal::fixture_admission(&replayed_permit)?;
    assert_eq!(
        scope
            .try_reserve_with_storage(
                request(&assignment, &initial_generation),
                replayed_permit,
                replayed_storage,
            )
            .await,
        Err(HostError::Journal)
    );

    let resumed_generation = "c".repeat(32);
    let resumed_permit = permit_with_generation(&resumed_generation);
    let resumed_storage = crate::journal::fixture_admission(&resumed_permit)?;
    assert_eq!(
        scope
            .try_reserve_with_storage(
                request(&assignment, &resumed_generation),
                resumed_permit,
                resumed_storage,
            )
            .await?,
        ReservationResult::Resumed { reservation_id }
    );
    let snapshot = scope
        .reservation(&assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        snapshot.initial_sample_generation_id.as_deref(),
        Some(initial_generation.as_str())
    );
    assert!(snapshot.resume_eligible);
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn uncreated_or_unreceipted_acquire_is_blocked_before_remote_effect() -> Result<(), HostError>
{
    let scratch = Scratch::new("effect-claim-resume")?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(limits(1, 2_000, 8_589_934_592))
        .await?;
    let scope = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let identity = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 33 })
        .await?;
    let ReservationResult::Reserved { .. } = reserve(&scope, &identity).await? else {
        return Err(HostError::Journal);
    };
    let (intent_id, fresh) = scope.begin_assigned_launch(&identity, "v33").await?;
    assert!(fresh);
    assert!(!scope.claim_assigned_acquire(intent_id).await?);

    let generation = "d".repeat(32);
    let permit = permit_with_generation(&generation);
    let storage = crate::journal::fixture_admission(&permit)?;
    assert_eq!(
        scope
            .try_reserve_with_storage(request(&identity, &generation), permit, storage,)
            .await?,
        ReservationResult::Resumed {
            reservation_id: scope
                .reservation(&identity)
                .await?
                .ok_or(HostError::Journal)?
                .reservation_id,
        }
    );
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn assignment_replay_is_connection_scoped_and_binding_is_immutable() -> Result<(), HostError>
{
    let scratch = Scratch::new("assignment-scope")?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(limits(2, 8_000, 17_179_869_184))
        .await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;

    let first_assignment = message_assignment(&first, 7).await?;
    let second_assignment = message_assignment(&second, 7).await?;
    let first_result = reserve(&first, &first_assignment).await?;
    let second_result = reserve(&second, &second_assignment).await?;
    let (
        ReservationResult::Reserved {
            reservation_id: first_id,
        },
        ReservationResult::Reserved {
            reservation_id: second_id,
        },
    ) = (first_result, second_result)
    else {
        return Err(HostError::Journal);
    };
    assert_ne!(first_id, second_id);
    let original_sample = first
        .reservation(&first_assignment)
        .await?
        .ok_or(HostError::Journal)?
        .initial_sample_generation_id;
    assert_eq!(
        reserve(&first, &first_assignment).await?,
        ReservationResult::Resumed {
            reservation_id: first_id,
        }
    );
    assert_eq!(
        reserve(&second, &second_assignment).await?,
        ReservationResult::Resumed {
            reservation_id: second_id,
        }
    );
    assert_eq!(
        first
            .reservation(&first_assignment)
            .await?
            .ok_or(HostError::Journal)?
            .initial_sample_generation_id,
        original_sample
    );
    assert_eq!(journal.occupied_reservation_count().await?, 2);

    let changed = connection("essential", 101, "donbeave/other-repo");
    assert_eq!(
        journal.connection_scope(&changed).await.err(),
        Some(HostError::Journal)
    );
    drop(first);
    drop(second);
    drop(journal);

    let journal = Journal::open(&scratch.file()).await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    let first_assignment = message_assignment(&first, 7).await?;
    let second_assignment = message_assignment(&second, 7).await?;
    assert_eq!(
        reserve(&first, &first_assignment).await?,
        ReservationResult::Resumed {
            reservation_id: first_id,
        }
    );
    assert_eq!(
        reserve(&second, &second_assignment).await?,
        ReservationResult::Resumed {
            reservation_id: second_id,
        }
    );
    assert_eq!(journal.occupied_reservation_count().await?, 2);
    Ok(())
}

#[tokio::test]
async fn concurrent_connections_cannot_exceed_one_job() -> Result<(), HostError> {
    let scratch = Scratch::new("max-one-race")?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(limits(1, 2_000, 8_589_934_592))
        .await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    let first_assignment = message_assignment(&first, 21).await?;
    let second_assignment = message_assignment(&second, 22).await?;

    let (left, right) = tokio::join!(
        reserve(&first, &first_assignment),
        reserve(&second, &second_assignment),
    );
    let left = left?;
    let right = right?;
    assert!(matches!(
        (left, right),
        (
            ReservationResult::Reserved { .. },
            ReservationResult::AtCapacity
        ) | (
            ReservationResult::AtCapacity,
            ReservationResult::Reserved { .. }
        )
    ));
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn uncertain_reservations_survive_restart_and_require_both_cleanup_proofs()
-> Result<(), HostError> {
    let scratch = Scratch::new("uncertain-restart")?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(limits(1, 2_000, 8_589_934_592))
        .await?;
    let scope = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let assignment = message_assignment(&scope, 25).await?;
    let ReservationResult::Reserved { reservation_id } = reserve(&scope, &assignment).await? else {
        return Err(HostError::Journal);
    };
    scope.mark_uncertain(reservation_id).await?;
    assert_eq!(
        reserve(&scope, &assignment).await?,
        ReservationResult::ReconcileRequired { reservation_id }
    );
    let snapshot = scope
        .reservation(&assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(snapshot.state, ReservationState::Uncertain);
    assert!(!snapshot.resume_eligible);
    assert!(!journal.is_drained().await?);
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    journal.configure_capacity(limits(1, 1, 1)).await?;
    let other = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    let other_assignment = message_assignment(&other, 26).await?;
    assert_eq!(
        reserve(&other, &other_assignment).await?,
        ReservationResult::AtCapacity
    );

    scope.record_remote_cleanup(reservation_id).await?;
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    scope.record_local_cleanup(reservation_id).await?;
    assert_eq!(journal.occupied_reservation_count().await?, 0);
    assert!(journal.is_drained().await?);
    assert_eq!(
        reserve(&scope, &assignment).await?,
        ReservationResult::Released { reservation_id }
    );
    Ok(())
}

#[tokio::test]
async fn current_schema_missing_reservation_table_fails_closed() -> Result<(), HostError> {
    let scratch = Scratch::new("missing-reservations")?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(limits(1, 2_000, 8_589_934_592))
        .await?;
    let scope = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let assignment = message_assignment(&scope, 28).await?;
    assert!(matches!(
        reserve(&scope, &assignment).await?,
        ReservationResult::Reserved { .. }
    ));
    drop(scope);
    drop(journal);

    let text = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE capacity_reservations", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}
