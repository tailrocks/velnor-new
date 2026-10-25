use super::*;

pub(super) async fn assert_pre_delivery_snapshot(
    setup: &bootstrap_start::CompositionSetup,
    reservation_id: i64,
    intent_id: i64,
    binding_digest: &str,
) -> Result<(), String> {
    let snapshot = setup
        .fixture
        .scope
        .native_publication_snapshot(reservation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The paired publication state is not durable.".to_owned())?;
    assert_eq!(snapshot.reservation_id, reservation_id);
    assert_eq!(snapshot.operation_id, intent_id);
    assert_eq!(snapshot.engine_id, setup.engine.engine_id());
    assert_eq!(
        snapshot.docker_root_digest,
        setup.engine.docker_root_digest()
    );
    assert_eq!(snapshot.instance_binding_digest, binding_digest);
    assert_eq!(snapshot.runner_container_id, RUNNER_ID);
    assert_eq!(snapshot.dind_container_id, DIND_ID);
    assert!(snapshot.resources_bound);
    assert_eq!(snapshot.jit_state, "bound");
    assert_eq!(snapshot.publication_state, "published");
    assert!(snapshot.projection_sha256.is_some());
    assert_eq!(snapshot.challenge_state.as_deref(), Some("published"));
    assert!(snapshot.challenge_digest.is_some());
    assert_eq!(snapshot.delivery_state, "not_requested");
    Ok(())
}
