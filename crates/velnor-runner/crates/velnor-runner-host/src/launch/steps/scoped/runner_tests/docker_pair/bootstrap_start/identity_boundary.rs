use super::*;

#[tokio::test]
async fn foreign_image_bundles_fail_before_worker_docker_effects() -> Result<(), String> {
    let setup = BootstrapSetup::open().await?;
    let before = setup.api.activity_snapshot()?;
    let pending = setup
        .prepared
        .storage()
        .pending_initialization()
        .ok_or_else(|| "The storage initializer is not ready for the mount handoff.".to_owned())?;

    let foreign_bundles = [
        crate::launch::verified_images::test_worker_images_for(
            "different-engine",
            setup.engine.docker_root_digest(),
            setup.engine.daemon_architecture(),
            setup.engine.platform(),
        ),
        crate::launch::verified_images::test_worker_images_for(
            setup.engine.engine_id(),
            setup.engine.docker_root_digest(),
            setup.engine.daemon_architecture(),
            "linux/arm64",
        ),
    ];

    for foreign in foreign_bundles {
        let result = crate::launch::bootstrap::start_bootstrap_pair(
            &setup.prepared,
            &setup.network,
            pending.metadata_handoff(),
            pending,
            &setup.api.docker,
            &setup.fixture.scope,
            &setup.engine,
            &foreign,
        )
        .await;
        if result.is_ok() {
            return Err("The production pair start accepted a foreign image bundle.".to_owned());
        }
        assert_eq!(setup.api.activity_snapshot()?, before);
    }
    Ok(())
}
