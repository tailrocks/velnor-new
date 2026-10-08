//! Reopen and race tests for the outer-network removal fence.

use std::num::NonZeroU32;

use crate::journal::{CapacityClaim, Journal, LaunchEffectState, OuterNetworkCleanupState};
use crate::{HostError, journal::tests::Scratch};

use super::{identity, reserve};

#[tokio::test]
async fn outer_network_removal_fences_worker_replay_after_reopen() -> Result<(), String> {
    let scratch =
        Scratch::new("launch-lifecycle-removal-fence").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let id = reserve(&journal, 8, 108).await?;
    journal
        .record_launch_effect_intent(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_outer_network_intent(id, "velnor-net-worker-abc")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .begin_outer_network_removal(id, "velnor-net-worker-abc", None)
        .await
        .map_err(|error| error.to_string())?;
    assert_fenced_worker_replays(&journal, id).await?;
    drop(journal);

    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened
            .outer_network_cleanup_state(id)
            .await
            .map_err(|error| error.to_string())?,
        OuterNetworkCleanupState::RemovalPending {
            name: "velnor-net-worker-abc".to_owned(),
            id: None,
        }
    );
    assert_fenced_worker_replays(&reopened, id).await?;
    let row = reopened
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or("launch row after fenced replay")?;
    assert_eq!(row.docker_id, None);
    assert_eq!(row.dind_id, None);
    assert_eq!(row.outer_network_id, None);
    assert_eq!(row.worker_volume, None);
    assert_eq!(row.launch_effect, LaunchEffectState::MayHaveEffect);
    let next = identity(9, 109).map_err(|error| error.to_string())?;
    let maximum = NonZeroU32::new(1).ok_or("capacity")?;
    assert_eq!(
        reopened.reserve_launch_if_accepting(&next, maximum).await,
        Ok(CapacityClaim::CapacityFull {
            occupied: 1,
            maximum,
        })
    );
    Ok(())
}

async fn assert_fenced_worker_replays(journal: &Journal, id: i64) -> Result<(), String> {
    assert_eq!(
        journal
            .bind_worker(id, Some("0a0b0c0d"), Some("1a1b1c1d"))
            .await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.bind(id, Some("0a0b0c0d"), None).await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.bind_worker_volume(id, "worker-volume").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.bind_outer_network_id(id, "a1b2c3d4").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.record_runner_start_intent(id, "0a0b0c0d").await,
        Err(HostError::Journal)
    );
    assert_eq!(
        journal.record_launch_effect_intent(id).await,
        Err(HostError::Journal)
    );
    Ok(())
}

#[tokio::test]
async fn outer_network_removal_and_worker_binding_are_serialized() -> Result<(), String> {
    let scratch =
        Scratch::new("launch-lifecycle-removal-race").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let id = reserve(&journal, 10, 110).await?;
    journal
        .record_launch_effect_intent(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_outer_network_intent(id, "velnor-net-worker-race")
        .await
        .map_err(|error| error.to_string())?;
    let (removal, bind) = tokio::join!(
        journal.begin_outer_network_removal(id, "velnor-net-worker-race", None),
        journal.bind_worker(id, Some("0a0b0c0d"), Some("1a1b1c1d")),
    );
    assert!(!(removal.is_ok() && bind.is_ok()));
    let state = journal
        .outer_network_cleanup_state(id)
        .await
        .map_err(|error| error.to_string())?;
    let row = journal
        .rows()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or("launch row after removal race")?;
    if matches!(state, OuterNetworkCleanupState::RemovalPending { .. }) {
        assert_eq!(row.docker_id, None);
        assert_eq!(row.dind_id, None);
    } else if bind.is_ok() {
        assert_eq!(row.docker_id.as_deref(), Some("0a0b0c0d"));
        assert_eq!(row.dind_id.as_deref(), Some("1a1b1c1d"));
    }
    Ok(())
}
