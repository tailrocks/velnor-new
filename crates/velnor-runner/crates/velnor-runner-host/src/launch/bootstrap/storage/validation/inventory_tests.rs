use crate::error::HostError;
use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};
use crate::launch::bootstrap::{test_observed_storage_attachment, test_observed_storage_role};

use super::validate_engine_metadata_views;

const PROBE_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const VOLUME_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BTRFS: u64 = 0x9123_683e;

fn fixture() -> Result<
    (
        Vec<crate::launch::bootstrap::ObservedStorageRole>,
        Vec<crate::launch::bootstrap::ObservedStorageAttachment>,
    ),
    HostError,
> {
    let role = test_observed_storage_role(
        StorageRole::EngineMetadata,
        "c".repeat(64),
        PROBE_DIGEST.to_owned(),
        StorageMedium::Disk,
        StorageFilesystemKind::Btrfs,
        (64 << 30, 60 << 30, 60 << 30),
        (Some(0), Some(0), Some(0)),
        StorageInodePolicy::DynamicBtrfs,
        (1 << 20, 1, 0),
        (0, 0),
    )?;
    let rows = [
        (
            StorageObserver::Probe,
            None,
            "/velnor/docker-root",
            "/",
            40,
            1_000,
            PROBE_DIGEST,
        ),
        (
            StorageObserver::Runner,
            Some("d".repeat(64)),
            "/.velnor-runtime-identity",
            "/volume/runtime-identity",
            41,
            1_001,
            VOLUME_DIGEST,
        ),
        (
            StorageObserver::Dind,
            Some("e".repeat(64)),
            "/.velnor-runtime-identity",
            "/volume/runtime-identity",
            51,
            1_001,
            VOLUME_DIGEST,
        ),
        (
            StorageObserver::Runner,
            Some("d".repeat(64)),
            "/opt/velnor/job-facts",
            "/volume/job-facts",
            42,
            1_002,
            VOLUME_DIGEST,
        ),
        (
            StorageObserver::Dind,
            Some("e".repeat(64)),
            "/opt/velnor/job-facts",
            "/volume/job-facts",
            52,
            1_002,
            VOLUME_DIGEST,
        ),
        (
            StorageObserver::Runner,
            Some("d".repeat(64)),
            "/opt/velnor/source-proof/control",
            "/volume/source-proof/control",
            43,
            1_003,
            VOLUME_DIGEST,
        ),
    ];
    let attachments = rows
        .into_iter()
        .map(
            |(observer, container, target, mount_root, mount_id, inode, digest)| {
                let contract = StorageRole::EngineMetadata
                    .attachments()
                    .iter()
                    .find(|item| item.observer == observer && item.target == target)
                    .ok_or(HostError::Journal)?;
                test_observed_storage_attachment(
                    StorageRole::EngineMetadata,
                    observer,
                    container,
                    target.to_owned(),
                    contract.source_target.map(str::to_owned),
                    mount_root.to_owned(),
                    contract.read_only,
                    mount_id,
                    inode,
                    if observer == StorageObserver::Probe {
                        (0, 41)
                    } else {
                        (0, 48)
                    },
                    if observer == StorageObserver::Probe {
                        (0, 41)
                    } else {
                        (0, 48)
                    },
                    BTRFS,
                    if observer == StorageObserver::Probe {
                        17
                    } else {
                        18
                    },
                    digest.to_owned(),
                )
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    Ok((vec![role], attachments))
}

#[test]
fn probe_capacity_identity_is_separate_from_metadata_volume_identity() -> Result<(), HostError> {
    let (roles, attachments) = fixture()?;
    validate_engine_metadata_views(&roles, &attachments)
}

#[test]
fn metadata_probe_cannot_substitute_for_capacity_root() -> Result<(), HostError> {
    let (mut roles, attachments) = fixture()?;
    roles[0].filesystem_instance_digest = VOLUME_DIGEST.to_owned();
    assert!(validate_engine_metadata_views(&roles, &attachments).is_err());
    Ok(())
}

#[test]
fn metadata_workers_reject_foreign_filesystem_and_child_identity() -> Result<(), HostError> {
    let (roles, mut attachments) = fixture()?;
    attachments[4].filesystem_id = 19;
    assert!(validate_engine_metadata_views(&roles, &attachments).is_err());

    let (roles, mut attachments) = fixture()?;
    attachments[4].statx_inode += 1;
    assert!(validate_engine_metadata_views(&roles, &attachments).is_err());
    Ok(())
}
