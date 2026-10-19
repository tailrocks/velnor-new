use super::support::{FakeLane, Fixture, exchange, fixture_storage};
use crate::journal::RequestAcquireState;
use crate::launch::resource_probe::StartPermit;
use crate::launch::steps::scoped::{acquire, runner};
use crate::scale_set::EnsureError;
use velnor_runner_github::Method;

#[tokio::test]
async fn acquire_401_is_not_replayed_and_keeps_the_physical_charge() -> Result<(), String> {
    let fixture = Fixture::open("scoped-acquire-401-no-replay").await?;
    let permit = StartPermit::test_fixture();
    let storage = fixture_storage(&permit)?;
    let physical = runner::reserve(&fixture.scope, permit, storage)
        .await
        .map_err(|error| error.to_string())?;
    let mut lane = FakeLane::new([
        Ok(exchange(401, "")),
        Ok(exchange(200, r#"{"count":1,"value":[41]}"#)),
    ]);

    assert_eq!(
        acquire::run(&mut lane, "queue-token", &fixture.scope, &fixture.request).await,
        Err(EnsureError::Uncertain)
    );
    assert_eq!(lane.methods, [Method::Post]);
    assert_eq!(lane.paths.len(), 1);
    assert!(lane.paths[0].ends_with("/acquirejobs"));
    assert_eq!(fixture.journal.occupied_reservation_count().await, Ok(1));
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.request)
            .await
            .map_err(|error| error.to_string())?,
        Some(RequestAcquireState::Uncertain)
    );
    assert!(
        fixture
            .scope
            .claim_runner_jit_for_request(
                physical.intent_id,
                physical.reservation_id,
                fixture
                    .request
                    .request_id()
                    .ok_or_else(|| "request ID is absent".to_owned())?,
                &physical.runner_name,
                physical.authority_generation,
                &physical.authority_digest,
            )
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}
