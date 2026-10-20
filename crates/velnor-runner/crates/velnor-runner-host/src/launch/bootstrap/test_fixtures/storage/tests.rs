use super::test_observed_storage_attachment;
use crate::journal::{StorageObserver, StorageRole};

const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;

#[test]
fn engine_metadata_attachment_binds_exact_docker_root_source() {
    let expected = test_observed_storage_attachment(
        StorageRole::EngineMetadata,
        StorageObserver::Probe,
        None,
        "/velnor/docker-root".to_owned(),
        Some("/var/lib/docker".to_owned()),
        "/".to_owned(),
        true,
        237,
        1237,
        (0, 38),
        (0, 41),
        BTRFS_SUPER_MAGIC,
        u64::MAX,
        "a".repeat(64),
    );
    assert!(expected.is_ok_and(|item| item.filesystem_id() == u64::MAX));

    let wrong_source = test_observed_storage_attachment(
        StorageRole::EngineMetadata,
        StorageObserver::Probe,
        None,
        "/velnor/docker-root".to_owned(),
        Some("/tmp/docker".to_owned()),
        "/".to_owned(),
        true,
        237,
        1237,
        (0, 38),
        (0, 41),
        BTRFS_SUPER_MAGIC,
        12,
        "a".repeat(64),
    );
    assert!(wrong_source.is_err());
}

#[test]
fn worker_attachment_cannot_name_a_host_source_path() {
    let attachment = test_observed_storage_attachment(
        StorageRole::RunnerRoot,
        StorageObserver::Runner,
        Some("b".repeat(64)),
        "/".to_owned(),
        Some("/var/lib/docker".to_owned()),
        "/".to_owned(),
        true,
        12,
        1012,
        (0, 41),
        (0, 41),
        BTRFS_SUPER_MAGIC,
        12,
        "a".repeat(64),
    );
    assert!(attachment.is_err());
}

#[test]
fn metadata_attachment_requires_its_exact_subpath_root() {
    let valid = test_observed_storage_attachment(
        StorageRole::EngineMetadata,
        StorageObserver::Runner,
        Some("b".repeat(64)),
        "/.velnor-runtime-identity".to_owned(),
        None,
        "/volume/runtime-identity".to_owned(),
        true,
        12,
        1012,
        (0, 41),
        (0, 41),
        BTRFS_SUPER_MAGIC,
        12,
        "a".repeat(64),
    );
    assert!(valid.is_ok());

    let wrong_root = test_observed_storage_attachment(
        StorageRole::EngineMetadata,
        StorageObserver::Runner,
        Some("b".repeat(64)),
        "/.velnor-runtime-identity".to_owned(),
        None,
        "/volume/job-facts".to_owned(),
        true,
        12,
        1012,
        (0, 41),
        (0, 41),
        BTRFS_SUPER_MAGIC,
        12,
        "a".repeat(64),
    );
    assert!(wrong_root.is_err());
}
