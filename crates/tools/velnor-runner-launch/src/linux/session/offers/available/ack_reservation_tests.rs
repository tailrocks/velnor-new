use std::num::NonZeroU32;

use velnor_runner_host::Journal;
use velnor_runner_journal::journal::{
    BoundBatchCapacityClaim, BoundCapacityClaim, JournalDockerDaemonBinding, LaunchEffectState,
    ReplayRoute, ScopedLaunchIdentity,
};

use crate::launch::harness::Scratch;

use super::{new_batch_launches, reserve_offer_batch};

fn route() -> ReplayRoute<'static> {
    ReplayRoute {
        destination: "https://api.github.com",
        registration_scope: "repository",
        owner: "acme",
        repository: "widget",
        runner_group_id: 7,
        runner_group_name: "trusted",
        scale_set_id: 9,
        scale_set_name: "linux",
    }
}

fn binding(engine: &str) -> Result<JournalDockerDaemonBinding, String> {
    JournalDockerDaemonBinding::new("/run/docker.sock", engine).map_err(|error| error.to_string())
}

async fn journal(label: &str) -> Result<(Scratch, Journal), String> {
    let scratch = Scratch::new(label).map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    Ok((scratch, journal))
}

#[tokio::test]
async fn capacity_fit_reserves_every_offer_and_exact_replay_is_not_launchable() -> Result<(), String>
{
    let (_scratch, journal) = journal("available-batch-fit").await?;
    let engine = binding("engine-a")?;
    let maximum = NonZeroU32::new(2).ok_or("nonzero maximum")?;
    let claims = reserve_offer_batch(
        &journal,
        route(),
        "session-a",
        41,
        &[7, 8],
        &engine,
        maximum,
    )
    .await
    .map_err(|error| error.to_string())?;
    let launches = new_batch_launches(&claims).map_err(|_| "fresh claims expected")?;
    assert_eq!(launches.len(), 2);

    let replay = reserve_offer_batch(
        &journal,
        route(),
        "session-a",
        41,
        &[7, 8],
        &engine,
        maximum,
    )
    .await
    .map_err(|error| error.to_string())?;
    let BoundBatchCapacityClaim::Offers(replay) = &replay else {
        return Err("exact replay did not return row claims".to_owned());
    };
    assert!(
        replay
            .iter()
            .all(|claim| matches!(claim, BoundCapacityClaim::Existing(_)))
    );
    assert!(new_batch_launches(&BoundBatchCapacityClaim::Offers(replay.clone())).is_err());
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn global_capacity_refusal_inserts_no_partial_available_rows() -> Result<(), String> {
    let (_scratch, journal) = journal("available-batch-full").await?;
    let engine = binding("engine-a")?;
    let occupied = ScopedLaunchIdentity::new(route(), "other-session", 3, 4)
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        journal
            .reserve_linux_launch_if_accepting(
                &occupied,
                &engine,
                NonZeroU32::new(2).ok_or("nonzero maximum")?,
            )
            .await
            .map_err(|error| error.to_string())?,
        BoundCapacityClaim::New(_)
    ));

    let result = reserve_offer_batch(
        &journal,
        route(),
        "session-a",
        41,
        &[7, 8],
        &engine,
        NonZeroU32::new(2).ok_or("nonzero maximum")?,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert!(matches!(
        result,
        BoundBatchCapacityClaim::CapacityFull { .. }
    ));
    assert!(new_batch_launches(&result).is_err());
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn mixed_replay_and_new_claims_are_not_authorized_as_a_batch() -> Result<(), String> {
    let (_scratch, journal) = journal("available-batch-mixed").await?;
    let engine = binding("engine-a")?;
    let maximum = NonZeroU32::new(3).ok_or("nonzero maximum")?;
    let first = reserve_offer_batch(&journal, route(), "session-a", 41, &[7], &engine, maximum)
        .await
        .map_err(|error| error.to_string())?;
    let first = new_batch_launches(&first).map_err(|_| "initial claim must be new")?;
    let original_id = first[0].id;

    let mixed = reserve_offer_batch(
        &journal,
        route(),
        "session-a",
        41,
        &[7, 8],
        &engine,
        maximum,
    )
    .await
    .map_err(|error| error.to_string())?;
    let BoundBatchCapacityClaim::Offers(claims) = &mixed else {
        return Err("mixed replay did not return row claims".to_owned());
    };
    assert!(matches!(claims[0], BoundCapacityClaim::Existing(id) if id == original_id));
    let BoundCapacityClaim::New(unstarted_id) = claims[1] else {
        return Err("second offer should be newly reserved".to_owned());
    };
    assert_eq!(new_batch_launches(&mixed), Err(vec![unstarted_id]));

    journal
        .record_launch_no_effect(unstarted_id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .launch_daemon_binding(original_id)
            .await
            .map_err(|error| error.to_string())?,
        Some(engine)
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].launch_effect, LaunchEffectState::NotStarted);
    assert_eq!(rows[1].launch_effect, LaunchEffectState::DefiniteNoEffect);
    Ok(())
}

#[tokio::test]
async fn duplicate_and_oversized_offer_sets_fail_before_any_row_is_created() -> Result<(), String> {
    let (_scratch, journal) = journal("available-batch-invalid").await?;
    let engine = binding("engine-a")?;
    let maximum = NonZeroU32::new(64).ok_or("nonzero maximum")?;
    assert!(
        reserve_offer_batch(
            &journal,
            route(),
            "session-a",
            41,
            &[7, 7],
            &engine,
            maximum
        )
        .await
        .is_err()
    );
    let oversized = (1..=51).collect::<Vec<_>>();
    assert!(
        reserve_offer_batch(
            &journal,
            route(),
            "session-a",
            41,
            &oversized,
            &engine,
            maximum,
        )
        .await
        .is_err()
    );
    let rows = journal.rows().await.map_err(|error| error.to_string())?;
    assert_eq!(rows.len(), 0);
    Ok(())
}
