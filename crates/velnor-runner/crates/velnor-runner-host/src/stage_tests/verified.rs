use super::super::launch::verified_images::{TestWorkerImageMutation, test_worker_images_with};
use super::*;

#[tokio::test]
async fn jit_stop_uses_verified_images_and_mounts_guest_metadata() -> Result<(), HostError> {
    let engine = Fake::new();
    let budget = test_resource_budget()?;
    let input = TestGuestImage::new()?;
    let partial = drive(
        &engine,
        "worker_a",
        budget,
        b"jit",
        PairStop::Jit,
        &input.connection,
        &input.images,
        &input.selected_engine,
        &input.file,
        StageAdmission::test_fixture(),
        &Forget,
    )
    .await?;
    assert!(partial.dind_id.is_some());
    assert!(partial.runner_id.is_some());
    assert_eq!(
        engine.events(),
        ["volumes", "create", "start", "create", "start", "jit"]
    );
    assert_eq!(engine.budgets(), [budget, budget]);
    let creates = engine.creates();
    assert_eq!(creates[0].image, input.images.dind().runtime_id());
    assert_eq!(creates[1].image, input.images.runner().runtime_id());
    assert_eq!(
        creates[1].bind_mounts[0].target,
        "/opt/velnor/guest-image.json"
    );
    assert!(creates[1].bind_mounts[0].read_only);
    Ok(())
}

#[tokio::test]
async fn invalid_verified_images_fail_before_recording_or_creating_resources()
-> Result<(), HostError> {
    let engine = Fake::new();
    let input = TestGuestImage::new()?;
    let invalid = test_worker_images_with(Some(TestWorkerImageMutation::Platform));
    assert_eq!(
        drive(
            &engine,
            "worker_a",
            test_resource_budget()?,
            b"jit",
            PairStop::Jit,
            &input.connection,
            &invalid,
            &input.selected_engine,
            &input.file,
            StageAdmission::test_fixture(),
            &Forget,
        )
        .await,
        Err(HostError::ForbiddenMount)
    );
    assert!(engine.events().is_empty());
    assert!(engine.creates().is_empty());
    Ok(())
}

#[tokio::test]
async fn images_bound_to_another_engine_fail_before_any_effect() -> Result<(), HostError> {
    let engine = Fake::new();
    let input = TestGuestImage::new()?;
    let other_engine = test_worker_images_with(Some(TestWorkerImageMutation::EngineId));
    assert_eq!(
        drive(
            &engine,
            "worker_a",
            test_resource_budget()?,
            b"jit",
            PairStop::Jit,
            &input.connection,
            &other_engine,
            &input.selected_engine,
            &input.file,
            StageAdmission::test_fixture(),
            &Forget,
        )
        .await,
        Err(HostError::Ownership)
    );
    assert!(engine.events().is_empty());
    assert!(engine.creates().is_empty());
    Ok(())
}
