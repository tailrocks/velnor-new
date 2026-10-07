//! Admission and durable drain serialize through the same SQLite writer lock.

use std::num::NonZeroU32;
use std::sync::Arc;

use crate::HostError;
use crate::journal::{CapacityClaim, Journal};

use super::admission::identity;
use crate::journal::tests::Scratch;

#[tokio::test]
async fn admission_racing_drain_is_either_reserved_first_or_rejected() -> Result<(), String> {
    let scratch = Scratch::new("capacity-drain-race").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let reserve_journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let drain_journal = Journal::open_existing(&path)
        .await
        .map_err(|error| error.to_string())?;
    let offer = identity("https://api.github.com", "one", "session", 1, 1)
        .map_err(|error| error.to_string())?;
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let maximum = NonZeroU32::new(1).ok_or("nonzero capacity")?;

    let reserve = async {
        barrier.clone().wait().await;
        reserve_journal
            .reserve_launch_if_accepting(&offer, maximum)
            .await
    };
    let drain = async {
        barrier.clone().wait().await;
        drain_journal.request_drain().await
    };
    let (claim, drain_result, _) = tokio::join!(reserve, drain, barrier.wait());
    let claim = claim.map_err(|error: HostError| error.to_string())?;
    drain_result.map_err(|error| error.to_string())?;

    let observer = Journal::open_readonly(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(observer.draining().await, Ok(true));
    let rows = observer.rows().await.map_err(|error| error.to_string())?;
    match claim {
        CapacityClaim::New(_) => assert_eq!(rows.len(), 1),
        CapacityClaim::Draining => assert_eq!(rows, Vec::new()),
        other => return Err(format!("unexpected admission result: {other:?}")),
    }
    Ok(())
}
