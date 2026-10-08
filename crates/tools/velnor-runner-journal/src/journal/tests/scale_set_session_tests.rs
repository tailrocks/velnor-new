//! The controller session intent is separate from worker-slot occupancy.

use crate::journal::{ReplayRoute, ScaleSetSessionClaim, ScaleSetSessionIdentity};
use crate::{DrainSnapshot, Journal};

use super::Scratch;

fn identity(set_id: i64) -> Result<ScaleSetSessionIdentity, crate::HostError> {
    ScaleSetSessionIdentity::new(
        ReplayRoute {
            destination: "https://api.github.com",
            registration_scope: "repository",
            owner: "acme",
            repository: "widget",
            runner_group_id: 2,
            runner_group_name: "trusted",
            scale_set_id: set_id,
            scale_set_name: "linux",
        },
        829_618_808,
        "acme/widget",
    )
}

#[tokio::test]
async fn singleton_session_intent_does_not_occupy_a_worker_slot() -> Result<(), String> {
    let scratch = Scratch::new("session-singleton").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let first = identity(3).map_err(|error| error.to_string())?;
    let changed_route = identity(4).map_err(|error| error.to_string())?;
    let ScaleSetSessionClaim::Reserved(id) = journal
        .reserve_scale_set_session_if_accepting(&first)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first controller session was not reserved".to_owned());
    };
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?,
        DrainSnapshot {
            draining: false,
            occupied_launches: 0,
            unresolved_intents: 1,
        }
    );
    assert_eq!(
        journal
            .reserve_scale_set_session_if_accepting(&changed_route)
            .await
            .map_err(|error| error.to_string())?,
        ScaleSetSessionClaim::Existing(id)
    );
    journal
        .record_scale_set_session_created(id, "session-opaque-1")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?,
        DrainSnapshot {
            draining: false,
            occupied_launches: 0,
            unresolved_intents: 1,
        },
        "the controller session remains unresolved but consumes no worker slot"
    );
    Ok(())
}

#[tokio::test]
async fn uncertain_session_create_survives_reopen_and_drain_fences_new_route() -> Result<(), String>
{
    let scratch = Scratch::new("session-reopen").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let first = identity(3).map_err(|error| error.to_string())?;
    let second = identity(4).map_err(|error| error.to_string())?;
    let intent = {
        let journal = Journal::open(&path)
            .await
            .map_err(|error| error.to_string())?;
        let ScaleSetSessionClaim::Reserved(id) = journal
            .reserve_scale_set_session_if_accepting(&first)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("first session did not reserve".to_owned());
        };
        id
    };
    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened
            .reserve_scale_set_session_if_accepting(&second)
            .await
            .map_err(|error| error.to_string())?,
        ScaleSetSessionClaim::Existing(intent),
        "an unrecorded create outcome must not be replayed after restart"
    );
    reopened
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened
            .reserve_scale_set_session_if_accepting(&second)
            .await
            .map_err(|error| error.to_string())?,
        ScaleSetSessionClaim::Existing(intent),
        "existing uncertainty remains visible under drain"
    );
    assert_eq!(
        reopened
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?,
        DrainSnapshot {
            draining: true,
            occupied_launches: 0,
            unresolved_intents: 1,
        }
    );
    Ok(())
}

#[tokio::test]
async fn definite_session_create_rejection_is_the_only_nonclose_release() -> Result<(), String> {
    let scratch = Scratch::new("session-rejected").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let route = identity(3).map_err(|error| error.to_string())?;
    let ScaleSetSessionClaim::Reserved(id) = journal
        .reserve_scale_set_session_if_accepting(&route)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("first session did not reserve".to_owned());
    };
    journal
        .record_scale_set_session_rejected(id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_scale_set_session_rejected(id)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?,
        DrainSnapshot {
            draining: false,
            occupied_launches: 0,
            unresolved_intents: 0,
        }
    );
    assert!(matches!(
        journal
            .reserve_scale_set_session_if_accepting(&route)
            .await
            .map_err(|error| error.to_string())?,
        ScaleSetSessionClaim::Reserved(_)
    ));
    Ok(())
}
