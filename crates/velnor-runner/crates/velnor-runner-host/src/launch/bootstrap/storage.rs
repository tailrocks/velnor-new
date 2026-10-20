mod attachment;
mod backing;
mod binding;
pub(crate) use attachment::ObservedStorageAttachment;
pub(crate) use backing::VerifiedRootBackingMapping;
pub(crate) use binding::{
    ObservedContainerCgroup, ObservedStorageRole, VerifiedStorageResourceBinding,
};

use crate::journal::{StorageObserver, StorageRole};

#[cfg(test)]
use velnor_resource_probe::cleanup_observer_wire::CleanupObserverBinding;

#[cfg(test)]
pub(crate) fn test_cleanup_observer_binding() -> CleanupObserverBinding {
    use velnor_resource_probe::cleanup_observer_wire::{
        CleanupDockerRootIdentity, CleanupVolume, ProcessIdentity,
    };
    let process = |pid, inode| ProcessIdentity {
        pid,
        start_ticks: 100,
        namespace_device: [0, 6],
        namespace_inode: inode,
    };
    CleanupObserverBinding {
        expected_guest_boot_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
        metadata_job_facts_inode: None,
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_id: 10,
            statx_mount_id: 10,
            mount_root: "/var/lib/docker".to_owned(),
            mount_device: [0, 7],
            statx_device: [0, 7],
            filesystem_type_magic: 0x9123_683e,
            filesystem_id: 1,
            read_only: true,
        },
        daemon: process(1, 10),
        runner: process(2, 20),
        dind: process(3, 30),
        cgroup_paths: ["/workers/runner".to_owned(), "/workers/dind".to_owned()],
        volume_mountpoints: ["shared-run", "runner-home", "runner-work", "dind-data"]
            .into_iter()
            .enumerate()
            .map(|(index, role)| CleanupVolume {
                role: role.to_owned(),
                mountpoint: format!("/var/lib/docker/volumes/test-{index}/_data"),
                mount_device: [0, index as u32 + 1],
                statx_device: [0, index as u32 + 1],
                inode: index as u64 + 1,
            })
            .collect(),
        owned_tmpfs_devices: [
            [0, 11],
            [0, 12],
            [0, 13],
            [0, 14],
            [0, 15],
            [0, 16],
            [0, 17],
            [0, 18],
        ],
    }
}

mod observation;
mod production;
mod validation;

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(
        role: StorageRole,
        observer: StorageObserver,
        container_id: Option<&str>,
        target: &str,
        inode: u64,
    ) -> ObservedStorageAttachment {
        ObservedStorageAttachment {
            role,
            observer,
            container_id: container_id.map(str::to_owned),
            target: target.to_owned(),
            source_target: None,
            volume_subpath: None,
            mount_root: "/".to_owned(),
            read_only: false,
            mount_id: 12,
            statx_inode: inode,
            device_major: 0,
            device_minor: 41,
            statx_device_major: 0,
            statx_device_minor: 41,
            fs_type_magic: 0x0102_1994,
            filesystem_id: 7,
            filesystem_instance_digest: "a".repeat(64),
        }
    }

    fn binding(attachments: Vec<ObservedStorageAttachment>) -> VerifiedStorageResourceBinding {
        VerifiedStorageResourceBinding {
            reservation_id: 1,
            operation_id: 2,
            engine_id: "engine".to_owned(),
            docker_root_digest: "b".repeat(64),
            sample_generation_id: "c".repeat(32),
            profile_generation_id: "d".repeat(32),
            profile_digest: "e".repeat(64),
            runner_container_id: "f".repeat(64),
            dind_container_id: "1".repeat(64),
            instance_binding_digest: "2".repeat(64),
            roles: Vec::new(),
            attachments,
            cgroups: Vec::new(),
            root_backings: Vec::new(),
            guest_boot_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
            cleanup_observer_binding: test_cleanup_observer_binding(),
        }
    }

    #[test]
    fn cleanup_root_accessor_returns_exact_home_root_inode() {
        let row = attachment(
            StorageRole::RunnerHome,
            StorageObserver::Runner,
            Some(&"f".repeat(64)),
            "/home/runner",
            73,
        );
        let binding = binding(vec![row]);
        let root = binding
            .cleanup_volume_root_attachment(StorageRole::RunnerHome)
            .expect("exact Home volume root");
        assert_eq!(root.target(), "/home/runner");
        assert_eq!(root.statx_inode(), 73);
        assert_eq!(root.device(), (0, 41));
    }

    #[test]
    fn cleanup_root_accessor_rejects_disagreeing_shared_run_views() {
        let runner = attachment(
            StorageRole::SharedRun,
            StorageObserver::Runner,
            Some(&"f".repeat(64)),
            "/run",
            73,
        );
        let dind = attachment(
            StorageRole::SharedRun,
            StorageObserver::Dind,
            Some(&"1".repeat(64)),
            "/run",
            74,
        );
        assert!(
            binding(vec![runner, dind])
                .cleanup_volume_root_attachment(StorageRole::SharedRun)
                .is_err()
        );
    }
}
pub(crate) use production::{PreparedTmpfsStorage, create_and_initialize};

#[cfg(test)]
#[path = "test_fixtures/storage.rs"]
mod test_fixtures;
pub(in crate::launch) use observation::observe_storage_resources_with_provider;

#[cfg(test)]
pub(crate) use test_fixtures::{
    test_observed_container_cgroup, test_observed_storage_attachment, test_observed_storage_role,
    test_verified_storage_resource_binding,
};
