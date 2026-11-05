// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn public_qualifier_rejects_incorrect_image_runtime_configuration() -> TestResult<()> {
    for (role, fault) in [
        (ImageRole::Runner, ImageFault::User),
        (ImageRole::Runner, ImageFault::MissingUser),
        (ImageRole::Runner, ImageFault::Entrypoint),
        (ImageRole::Runner, ImageFault::MissingEntrypoint),
        (ImageRole::Dind, ImageFault::User),
        (ImageRole::Dind, ImageFault::Entrypoint),
        (ImageRole::Dind, ImageFault::MissingEntrypoint),
    ] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.image_fault = Some((role, fault));
        plan.foreign_sentinel = true;
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert!(!output.status.success(), "accepted {role:?} {fault:?}");
        assert!(state.fault_reached);
        assert_eq!(state.volume_creations, 0);
        assert!(state.created.is_empty());
        assert!(state.foreign_container_present && state.foreign_volume_present);
    }
    Ok(())
}

#[test]
fn public_qualifier_rejects_incorrect_private_daemon_storage() -> TestResult<()> {
    for (root, driver) in [("/wrong", "vfs"), ("/var/lib/docker", "overlay2")] {
        let git = GitFixture::new()?;
        let mut plan = ApiPlan::new(git.source_sha());
        plan.inner_root = root.to_owned();
        plan.inner_driver = driver.to_owned();
        plan.foreign_sentinel = true;
        let (output, state) = qualify(&git, git.source_sha(), plan)?;
        assert!(!output.status.success(), "accepted {root} {driver}");
        assert!(state.readiness_attempts > 0);
        assert!(state.foreign_container_present && state.foreign_volume_present);
        assert_eq!(state.containers.len(), 1);
        assert_eq!(state.volumes, ["foreign-sentinel"]);
    }
    Ok(())
}
