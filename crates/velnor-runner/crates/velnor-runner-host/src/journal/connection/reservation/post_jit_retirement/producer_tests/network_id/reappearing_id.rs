use crate::error::HostError;

use super::{TestDocker, assert_remove_state, prepare_case, run_on_production_stack};

#[test]
fn retirement_rejects_saved_id_reappearing_after_initial_absence() -> Result<(), HostError> {
    run_on_production_stack(retirement_rejects_saved_id_reappearing_after_initial_absence_inner)
}

async fn retirement_rejects_saved_id_reappearing_after_initial_absence_inner()
-> Result<(), HostError> {
    let (fixture, claim, selected, name, saved_id, labels) = prepare_case().await?;
    let docker = TestDocker::open(selected.engine_id(), &name, &saved_id, &labels, false, None)?;
    docker.reappear_on_second_saved_id_inspection().await;

    assert!(
        crate::launch::bootstrap::remove_owned_network_for_retirement(
            &docker.docker,
            &fixture.scope,
            &selected,
            fixture.reservation_id,
            &claim,
        )
        .await
        .is_err()
    );
    assert_eq!(docker.deleted_id().await, None);
    assert!(docker.network_is_present().await);
    assert_remove_state(&fixture, "uncertain").await
}
