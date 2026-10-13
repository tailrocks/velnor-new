//! Assignment reservations participate in the production poll admission gate.

use velnor_runner_github::Poll;

use crate::Outcome;
use crate::journal::LaunchReservation;
use crate::launch::Admit;
use crate::launch::inspect_tests::DockerStub;
use crate::launch_harness::{available, open};

use super::admission;

#[tokio::test]
async fn full_capacity_replay_of_existing_assignment_is_admitted() -> Result<(), String> {
    let (_scratch, journal) = open("admission-existing").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 1)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new assignment reservation".to_owned());
    };
    journal
        .bind_pair(id, "runner-42", "dind-42")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;

    let admission = admission(&journal, 1, 1, 1, 0, 0, &batch(101, 42))
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(admission.reservation, Some(LaunchReservation::Existing(id)));
    assert_eq!(admission.decision, Admit::Start { stop: true });
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn free_capacity_creates_and_admits_a_new_assignment() -> Result<(), String> {
    let (_scratch, journal) = open("admission-new").await?;

    let admission = admission(&journal, 1, 1, 1, 0, 0, &batch(100, 42))
        .await
        .map_err(|error| error.to_string())?;

    let Some(LaunchReservation::New(id)) = admission.reservation else {
        return Err("expected a new assignment reservation".to_owned());
    };
    assert_eq!(admission.decision, Admit::Start { stop: true });
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    assert_eq!(journal.rows().await.map_err(|e| e.to_string())?[0].id, id);
    Ok(())
}

#[tokio::test]
async fn full_capacity_rejects_an_unrelated_assignment() -> Result<(), String> {
    let (_scratch, journal) = open("admission-unrelated").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 1)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected an occupying assignment".to_owned());
    };

    let admission = admission(&journal, 1, 1, 1, 0, 0, &batch(101, 43))
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(admission.reservation, Some(LaunchReservation::AtCapacity));
    assert_eq!(admission.decision, Admit::Hold);
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    assert_eq!(journal.rows().await.map_err(|e| e.to_string())?.len(), 1);
    assert_eq!(
        journal.intent(id).await.map_err(|e| e.to_string())?.subject,
        "s1:42"
    );
    Ok(())
}

#[tokio::test]
async fn unresolved_assignment_reservation_blocks_initial_scale_mint() -> Result<(), String> {
    let (_scratch, journal) = open("scale-reserved-assignment").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(1, 42, 100, 2)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new assignment reservation".to_owned());
    };
    journal
        .finish(id, Outcome::Uncertain)
        .await
        .map_err(|error| error.to_string())?;
    let docker = DockerStub::open(Vec::new())?;
    let mut scaled = false;

    let result = super::scale_if_free(&journal, &docker.docker, 2, 1, || {
        scaled = true;
        async { Ok(None) }
    })
    .await
    .map_err(|error| error.to_string())?;
    docker.finish().await?;

    assert_eq!(result, None);
    assert!(
        !scaled,
        "occupied assignment reservation must block scaling"
    );
    assert_eq!(
        journal
            .occupied_launches()
            .await
            .map_err(|e| e.to_string())?,
        1
    );
    Ok(())
}

fn batch(message_id: i64, request_id: i64) -> Poll {
    let mut polled = available(&[request_id]);
    if let Poll::Batch(batch) = &mut polled {
        batch.message_id = message_id;
    }
    polled
}
