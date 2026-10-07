//! Exact replay and cross-scope identity tests.

use std::num::NonZeroU32;

use crate::HostError;
use crate::journal::{CapacityClaim, Journal, ReplayRoute, ScopedLaunchIdentity};

use super::admission::identity;
use crate::journal::tests::Scratch;

#[tokio::test]
async fn exact_replay_is_idempotent_and_route_session_scopes_are_distinct() -> Result<(), String> {
    let scratch = Scratch::new("capacity-replay").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let first = identity("https://api.github.com", "one", "full-session-a", 9, 22)
        .map_err(|error| error.to_string())?;
    let same = identity("https://api.github.com", "one", "full-session-a", 9, 22)
        .map_err(|error| error.to_string())?;
    let other_session = identity("https://api.github.com", "one", "full-session-b", 9, 22)
        .map_err(|error| error.to_string())?;
    let other_repository = identity("https://api.github.com", "two", "full-session-a", 9, 22)
        .map_err(|error| error.to_string())?;
    let limit = NonZeroU32::new(3).ok_or("nonzero limit")?;

    let new = journal
        .reserve_launch_if_accepting(&first, limit)
        .await
        .map_err(|error| error.to_string())?;
    let CapacityClaim::New(id) = new else {
        return Err(format!("expected new reservation, got {new:?}"));
    };
    assert_eq!(
        journal
            .reserve_launch_if_accepting(&same, limit)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::Existing(id)
    );
    assert!(matches!(
        journal
            .reserve_launch_if_accepting(&other_session, limit)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::New(_)
    ));
    assert!(matches!(
        journal
            .reserve_launch_if_accepting(&other_repository, limit)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::New(_)
    ));
    assert_eq!(
        journal
            .rows()
            .await
            .map_err(|error| error.to_string())?
            .len(),
        3
    );
    Ok(())
}

#[test]
fn route_identity_accepts_organization_scope_and_rejects_invalid_ids() {
    let route = ReplayRoute {
        destination: "https://github.example.test/api/v3",
        registration_scope: "organization",
        owner: "example-org",
        repository: "",
        runner_group_id: 17,
        runner_group_name: "restricted",
        scale_set_id: 31,
        scale_set_name: "linux-private",
    };
    assert!(ScopedLaunchIdentity::new(route, "session", 1, 2).is_ok());
    assert_eq!(
        ScopedLaunchIdentity::new(route, "session", -1, 2),
        Err(HostError::Journal)
    );
    assert_eq!(
        ScopedLaunchIdentity::new(route, "session", 1, 0),
        Err(HostError::Journal)
    );
    let invalid_destination = ReplayRoute {
        destination: "http://github.example.test",
        ..route
    };
    assert_eq!(
        ScopedLaunchIdentity::new(invalid_destination, "session", 1, 2),
        Err(HostError::Journal)
    );
}
