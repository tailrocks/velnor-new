use super::{ExpectedPublicationFile, VerifiedMetadataVolume, lower_hex, verify_record};
use sha2::{Digest, Sha256};
use velnor_resource_probe::{
    execution_channel_wire::ExecutionChannelDirectory,
    execution_publication::{
        ExecutionPublicationFile, ExecutionPublicationKind, ExecutionPublicationRecord,
    },
};

const OP: &str = "0123456789abcdef0123456789abcdef";
const BOOT: &str = "boot-identity";
const MOUNTPOINT: &str = "/var/lib/docker/volumes/velnor-meta/_data";
const KERNEL_SOURCE: &str = "/dev/vda";
const KERNEL_ROOT: &str = "/var/lib/docker/volumes/velnor-meta/_data";
const PAYLOAD: &[u8] = b"{\"schema_version\":1}";

fn directory() -> ExecutionChannelDirectory {
    ExecutionChannelDirectory {
        target: "/opt/velnor/source-proof/control".to_owned(),
        inode: 71,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 13,
        statx_mount_id: 13,
        mount_root: format!("{KERNEL_ROOT}/source-proof/control"),
        mount_source: KERNEL_SOURCE.to_owned(),
        mount_device_major: 8,
        mount_device_minor: 1,
        statx_device_major: 8,
        statx_device_minor: 1,
        filesystem_type_magic: 0xef53,
        filesystem_type_name: "ext4".to_owned(),
        filesystem_id: 22,
        read_only: true,
        executable_mount: false,
    }
}

fn record() -> ExecutionPublicationRecord {
    let digest = lower_hex(&Sha256::digest(PAYLOAD));
    ExecutionPublicationRecord {
        schema_version: 1,
        operation_id: OP.to_owned(),
        guest_boot_id: BOOT.to_owned(),
        mount_namespace_id: 401,
        kind: ExecutionPublicationKind::Challenge,
        payload_sha256: digest.clone(),
        file: ExecutionPublicationFile {
            path: "/opt/velnor/source-proof/control/challenge.json".to_owned(),
            control_directory_inode: 71,
            inode: 72,
            bytes: PAYLOAD.len() as u64,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0o444,
            mount_id: 801,
            statx_mount_id: 801,
            mount_device_major: 8,
            mount_device_minor: 1,
            statx_device_major: 8,
            statx_device_minor: 1,
            filesystem_type_magic: 0xef53,
            filesystem_id: 22,
            mount_root: KERNEL_ROOT.to_owned(),
            mount_source: KERNEL_SOURCE.to_owned(),
            readback_sha256: digest,
        },
    }
}

fn volume() -> VerifiedMetadataVolume {
    VerifiedMetadataVolume {
        name: "velnor-meta".to_owned(),
        id: "velnor-meta".to_owned(),
        mountpoint: MOUNTPOINT.to_owned(),
    }
}

fn expected_file<'a>(
    expected_directory: &'a ExecutionChannelDirectory,
    volume: &'a VerifiedMetadataVolume,
    payload: &'a [u8],
    expected_digest: &'a str,
) -> ExpectedPublicationFile<'a> {
    ExpectedPublicationFile {
        kind: ExecutionPublicationKind::Challenge,
        expected_path: "/opt/velnor/source-proof/control/challenge.json",
        helper_operation_id: OP,
        guest_boot_id: BOOT,
        payload,
        expected_digest,
        expected_directory,
        volume,
    }
}

#[test]
fn record_accepts_the_same_volume_with_a_private_mount_namespace_id() {
    let expected = directory();
    let volume = volume();
    let digest = lower_hex(&Sha256::digest(PAYLOAD));
    let accepted = verify_record(
        record(),
        &expected_file(&expected, &volume, PAYLOAD, &digest),
    );
    assert!(accepted.is_ok());
}

#[test]
fn record_rejects_a_different_volume_directory_or_payload() {
    let expected = directory();
    let volume = volume();
    let digest = lower_hex(&Sha256::digest(PAYLOAD));
    let mut wrong_directory = record();
    wrong_directory.file.control_directory_inode += 1;
    assert!(
        verify_record(
            wrong_directory,
            &expected_file(&expected, &volume, PAYLOAD, &digest)
        )
        .is_err()
    );
    let mut wrong_volume = record();
    wrong_volume.file.mount_source.push_str("-foreign");
    assert!(
        verify_record(
            wrong_volume,
            &expected_file(&expected, &volume, PAYLOAD, &digest)
        )
        .is_err()
    );
    let wrong_digest = "0".repeat(64);
    assert!(
        verify_record(
            record(),
            &expected_file(&expected, &volume, PAYLOAD, &wrong_digest)
        )
        .is_err()
    );
}

#[test]
fn record_accepts_rooted_and_nested_kernel_mount_roots() {
    for (publisher_root, runner_root) in [
        ("/", "/source-proof/control"),
        (
            "/var/lib/docker/volumes/velnor-meta/_data",
            "/var/lib/docker/volumes/velnor-meta/_data/source-proof/control",
        ),
    ] {
        let mut expected = directory();
        expected.mount_root = runner_root.to_owned();
        let mut actual = record();
        actual.file.mount_root = publisher_root.to_owned();
        let volume = volume();
        let digest = lower_hex(&Sha256::digest(PAYLOAD));
        assert!(
            verify_record(actual, &expected_file(&expected, &volume, PAYLOAD, &digest)).is_ok()
        );
    }
}

#[test]
fn record_rejects_wrong_control_subpath_or_noncanonical_kernel_root() {
    for publisher_root in [
        "/var/lib/docker/volumes/foreign/_data",
        "/var/lib/docker/volumes/velnor-meta/_data/../foreign",
        "/var/lib/docker/volumes/velnor-meta/_data/",
    ] {
        let mut actual = record();
        actual.file.mount_root = publisher_root.to_owned();
        assert!(
            verify_record(
                actual,
                &expected_file(
                    &directory(),
                    &volume(),
                    PAYLOAD,
                    &lower_hex(&Sha256::digest(PAYLOAD)),
                ),
            )
            .is_err()
        );
    }
}
