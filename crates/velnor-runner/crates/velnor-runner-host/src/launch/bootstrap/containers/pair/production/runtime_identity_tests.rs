use super::{require_worker_runtime_id, worker_image};
use crate::launch::bootstrap::BootstrapContainerRole;
use crate::launch::verified_images::{TestWorkerImageMutation, test_worker_images_with};

#[test]
fn runtime_identity_uses_the_selected_role_and_rejects_config_substitution() {
    let images = test_worker_images_with(None);
    for (role, other_role) in [
        (BootstrapContainerRole::Runner, BootstrapContainerRole::Dind),
        (BootstrapContainerRole::Dind, BootstrapContainerRole::Runner),
    ] {
        let identity = worker_image(&images, role);
        let other_identity = worker_image(&images, other_role);
        assert_ne!(identity.runtime_id(), other_identity.runtime_id());
        assert_eq!(
            require_worker_runtime_id(Some(identity.runtime_id()), role, &images),
            Ok(())
        );
        assert!(
            require_worker_runtime_id(Some(other_identity.runtime_id()), role, &images).is_err()
        );
        assert!(require_worker_runtime_id(Some(identity.config_digest()), role, &images).is_err());
        assert!(require_worker_runtime_id(None, role, &images).is_err());
    }
}

#[test]
fn index_manifest_and_classic_stores_use_their_verified_runtime_ids() {
    for mutation in [
        None,
        Some(TestWorkerImageMutation::RunnerManifestStore),
        Some(TestWorkerImageMutation::RunnerClassicStore),
    ] {
        let images = test_worker_images_with(mutation);
        let identity = worker_image(&images, BootstrapContainerRole::Runner);
        assert_eq!(
            require_worker_runtime_id(
                Some(identity.runtime_id()),
                BootstrapContainerRole::Runner,
                &images
            ),
            Ok(())
        );
        if identity.runtime_id() != identity.config_digest() {
            assert!(
                require_worker_runtime_id(
                    Some(identity.config_digest()),
                    BootstrapContainerRole::Runner,
                    &images
                )
                .is_err()
            );
        }
    }
}
