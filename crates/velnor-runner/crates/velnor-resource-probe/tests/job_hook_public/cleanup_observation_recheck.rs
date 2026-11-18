// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use velnor_resource_probe::{
    ProbeError,
    cleanup_observation::{
        CleanupObservationRequest, GuestDockerRootIdentity, GuestObservation, observe_cleanup_in,
    },
};

use super::{ScopedTempDir, TestResult};

const BOOT_ID: &str = "0123abcd-0123-4abc-8def-0123456789ab";

fn guest() -> GuestObservation {
    GuestObservation {
        boot_id: BOOT_ID.to_owned(),
        docker_root: GuestDockerRootIdentity {
            target: "/var/lib/docker".to_owned(),
            mount_id: 1,
            statx_mount_id: 1,
            mount_root: "/".to_owned(),
            mount_device: [8, 1],
            statx_device: [8, 1],
            filesystem_type_magic: 1,
            filesystem_id: 1,
            read_only: false,
        },
    }
}

fn request() -> CleanupObservationRequest {
    CleanupObservationRequest {
        schema_version: 1,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        expected_guest_boot_id: BOOT_ID.to_owned(),
        cgroup_paths: ["/velnor-a".to_owned(), "/velnor-b".to_owned()],
    }
}

#[test]
fn cgroup_created_while_guest_identity_is_reread_is_not_reported_absent() -> TestResult<()> {
    let root = ScopedTempDir::new("cleanup-recheck")?;
    let created = root.path().join("velnor-b");
    let mut observations = 0_u8;
    let result = observe_cleanup_in(request(), root.path(), || {
        observations += 1;
        if observations == 2 {
            fs::create_dir(&created).map_err(|error| ProbeError::Read("test_create", error))?;
        }
        Ok(guest())
    });
    assert!(matches!(
        result,
        Err(ProbeError::Invalid("cleanup_cgroup_present"))
    ));
    Ok(())
}

#[test]
fn cgroups_absent_throughout_are_reported_absent() -> TestResult<()> {
    let root = ScopedTempDir::new("cleanup-absent")?;
    let observation = observe_cleanup_in(request(), root.path(), || Ok(guest()))?;
    assert_eq!(observation.absent_cgroup_paths, request().cgroup_paths);
    Ok(())
}
