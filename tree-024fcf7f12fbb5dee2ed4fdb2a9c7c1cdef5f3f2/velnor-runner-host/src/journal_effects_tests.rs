use crate::journal::LaunchReservation;
use crate::{HostError, Journal};

#[tokio::test]
async fn acquired_and_jit_transitions_cannot_be_replayed() -> Result<(), HostError> {
    let scratch = crate::launch_harness::Scratch::new("effect-claims")?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.bind_engine("docker-engine-test").await?;
    let LaunchReservation::New(id) = journal.reserve_assignment(7, 42, 100, 1).await? else {
        return Err(HostError::Journal);
    };
    assert!(journal.claim_acquire(id).await?);
    assert!(!journal.claim_acquire(id).await?);
    journal.resolve_acquire(id, true).await?;
    assert_eq!(
        journal.resolve_acquire(id, false).await,
        Err(HostError::Journal)
    );
    assert!(journal.claim_jit(id).await?);
    assert!(!journal.claim_jit(id).await?);
    let row = journal.intent(id).await?;
    assert!(row.acquire_attempted);
    assert!(row.acquire_resolved);
    assert!(row.acquired);
    assert!(row.jit_requested);
    assert_eq!(journal.occupied_launches().await?, 1);
    Ok(())
}
