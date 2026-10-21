use crate::error::HostError;
use crate::journal::connection::reservation::post_jit_retirement::test_support;
use crate::journal::request_lifecycle::tests::native_started_intake_fixture;
use crate::launch::bootstrap::{test_verified_storage_cleanup, verify_storage_cleanup_for_scope};

#[tokio::test]
async fn foreign_reservation_cleanup_proof_cannot_advance_retirement() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    test_support::recover_fixture(&fixture).await?;
    let claim = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    let actual =
        verify_storage_cleanup_for_scope(&fixture.scope, &fixture.journal, fixture.reservation_id)
            .await?;
    let foreign = test_verified_storage_cleanup(
        fixture.reservation_id + 1,
        actual.operation_id(),
        actual.engine_id().to_owned(),
        actual.docker_root_digest().to_owned(),
        actual.runner_container_id().to_owned(),
        actual.dind_container_id().to_owned(),
        actual.volume_names().to_vec(),
        actual.instance_binding_digest().to_owned(),
        actual.role_inventory().to_vec(),
        true,
        true,
        true,
        true,
    )?;

    assert!(matches!(
        fixture
            .scope
            .record_retirement_local_absence(&claim, foreign)
            .await,
        Err(HostError::Identity)
    ));
    super::assert_retirement_storage_state(&fixture.journal, fixture.reservation_id, 0).await
}
