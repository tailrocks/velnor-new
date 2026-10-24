use crate::{error::HostError, launch::bootstrap::cleanup::runtime_tests::cleanup_runtime_fixture};

#[test]
fn cleanup_binding_rejects_changed_docker_root_device() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "cleanup_binding_rejects_changed_docker_root_device",
        || {
            Box::pin(async {
                cleanup_binding_rejects_changed_docker_root_device_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn cleanup_binding_rejects_changed_docker_root_device_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = cleanup_runtime_fixture().await?;
    let identity = fixture
        .scope
        .storage_cleanup_identity(fixture.reservation_id)
        .await?
        .ok_or(HostError::Ownership)?;
    let witness = fixture
        .scope
        .storage_cleanup_witness(fixture.reservation_id)
        .await?
        .ok_or(HostError::Ownership)?;
    let binding = witness.binding();
    assert!(super::validate_docker_root(&identity, binding).is_ok());

    let mut changed = binding.clone();
    changed.docker_root.statx_device[1] += 1;
    assert!(super::validate_docker_root(&identity, &changed).is_err());
    Ok(())
}

#[test]
fn cleanup_binding_rejects_changed_nonmetadata_volume_device() -> Result<(), String> {
    crate::launch::test_harness_stack::run_on_test_harness_stack(
        "cleanup_binding_rejects_changed_nonmetadata_volume_device",
        || {
            Box::pin(async {
                cleanup_binding_rejects_changed_nonmetadata_volume_device_on_fixture_stack()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn cleanup_binding_rejects_changed_nonmetadata_volume_device_on_fixture_stack()
-> Result<(), HostError> {
    let fixture = cleanup_runtime_fixture().await?;
    let identity = fixture
        .scope
        .storage_cleanup_identity(fixture.reservation_id)
        .await?
        .ok_or(HostError::Ownership)?;
    let witness = fixture
        .scope
        .storage_cleanup_witness(fixture.reservation_id)
        .await?
        .ok_or(HostError::Ownership)?;
    let binding = witness.binding();
    assert!(super::validate_volume_attachments(&identity, binding).is_ok());

    let mut changed = binding.clone();
    let volume = changed
        .volume_mountpoints
        .iter_mut()
        .find(|volume| volume.role == "shared-run")
        .ok_or(HostError::Ownership)?;
    volume.mount_device[1] += 1;
    assert!(super::validate_volume_attachments(&identity, &changed).is_err());
    Ok(())
}
