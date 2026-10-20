use super::{PublicationRecoveryProjection, lower_hex};
use sha2::{Digest, Sha256};
use velnor_resource_probe::{
    execution_channel_wire::ExecutionChannelDirectory,
    job_facts_wire::JobFactsRuntimeIdentityDirectory,
};

fn recovery() -> PublicationRecoveryProjection {
    let operation_id = "0123456789abcdef0123456789abcdef".to_owned();
    let image = format!("sha256:{}", "a".repeat(64));
    let name = format!("velnor-execution-publication-{operation_id}");
    let options = bollard::query_parameters::CreateContainerOptions {
        name: Some(name.clone()),
        platform: "linux/amd64".to_owned(),
    };
    let config = bollard::models::ContainerCreateBody {
        image: Some(image.clone()),
        ..Default::default()
    };
    let projection_digest = lower_hex(&Sha256::digest(
        crate::launch::resource_probe::projection::canonical::pair(&options, &config)
            .expect("projection"),
    ));
    let root = super::super::DockerRoot::parse("/var/lib/docker").expect("Docker root");
    PublicationRecoveryProjection {
        schema_version: 2,
        operation_id,
        instance_id: "b".repeat(32),
        engine_id: "selected-engine".to_owned(),
        docker_root_digest: root.digest().to_owned(),
        operation_name: name,
        platform: "linux/amd64".to_owned(),
        source_revision: "c".repeat(40),
        runtime_image_id: image,
        image_binding_digest: "d".repeat(64),
        projection_digest,
        metadata_volume_name: "metadata-owned".to_owned(),
        metadata_volume_id: "metadata-owned".to_owned(),
        metadata_volume_mountpoint: "/var/lib/docker/volumes/metadata-owned/_data".to_owned(),
        expected_payload_sha256: "e".repeat(64),
        expected_guest_image_sha256: None,
        expected_runtime_identity: None,
        parent_kind: "challenge".to_owned(),
        parent_operation_id: 1,
        reservation_id: 2,
        connection_id: "private-connection".to_owned(),
        runner_id: 3,
        runner_container_id: "1".repeat(64),
        dind_container_id: "2".repeat(64),
        metadata_layout_version: 2,
        metadata_layout_digest: "f".repeat(64),
        guest_boot_id: "123e4567-e89b-12d3-a456-426614174000".to_owned(),
        publisher_mode: "execution_publication".to_owned(),
        runner_directory: ExecutionChannelDirectory {
            target: "/opt/velnor/source-proof/control".to_owned(),
            inode: 11,
            owner_uid: 0,
            owner_gid: 0,
            mode: 0o555,
            mount_id: 12,
            statx_mount_id: 12,
            mount_root: "/source-proof/control".to_owned(),
            mount_source: "/dev/vda".to_owned(),
            mount_device_major: 8,
            mount_device_minor: 0,
            statx_device_major: 8,
            statx_device_minor: 0,
            filesystem_type_magic: 0xef53,
            filesystem_type_name: "ext4".to_owned(),
            filesystem_id: 13,
            read_only: true,
            executable_mount: false,
        },
        config,
        expected_config: bollard::models::ContainerConfig::default(),
    }
}

#[test]
fn recovery_projection_is_canonical_bounded_and_tamper_evident() {
    let value = recovery();
    let encoded = value.encode().expect("bounded projection");
    let sha = lower_hex(&Sha256::digest(encoded.as_bytes()));
    let decoded = PublicationRecoveryProjection::decode(&encoded, &sha)
        .expect("same projection after restart");
    assert_eq!(decoded.encode().expect("canonical bytes"), encoded);
    assert!(PublicationRecoveryProjection::decode(&encoded, &"0".repeat(64)).is_err());

    let tampered = encoded.replace("metadata-owned", "foreign-volume");
    assert!(PublicationRecoveryProjection::decode(&tampered, &sha).is_err());
}

#[test]
fn recovery_projection_rejects_bad_volume_and_parent_facts() {
    let mut value = recovery();
    value.metadata_volume_id = "foreign-volume".to_owned();
    assert!(value.encode().is_err());

    let mut value = recovery();
    value.dind_container_id = value.runner_container_id.clone();
    assert!(value.encode().is_err());
}

#[test]
fn job_facts_recovery_keeps_fixed_writer_mode_and_mount() {
    let mut value = recovery();
    value.parent_kind = "job-facts".to_owned();
    value.publisher_mode = "job_facts_publication".to_owned();
    value.runner_directory.target = "/opt/velnor/job-facts".to_owned();
    value.runner_directory.mount_root = "/job-facts".to_owned();
    value.expected_guest_image_sha256 = Some("a".repeat(64));
    value.expected_runtime_identity = Some(JobFactsRuntimeIdentityDirectory {
        device_major: 8,
        device_minor: 0,
        inode: 17,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
    });
    value.config.cmd = Some(vec!["--mode".to_owned(), value.publisher_mode.clone()]);
    let options = bollard::query_parameters::CreateContainerOptions {
        name: Some(value.operation_name.clone()),
        platform: value.platform.clone(),
    };
    value.projection_digest = lower_hex(&Sha256::digest(
        crate::launch::resource_probe::projection::canonical::pair(&options, &value.config)
            .expect("job facts projection bytes"),
    ));

    let encoded = value.encode().expect("valid JobFacts recovery projection");
    let sha = lower_hex(&Sha256::digest(encoded.as_bytes()));
    let restored = PublicationRecoveryProjection::decode(&encoded, &sha)
        .expect("JobFacts recovery projection after restart");
    let projected = restored
        .into_projection("/var/lib/docker")
        .expect("fixed JobFacts publisher projection");
    assert_eq!(projected.publisher_mode, "job_facts_publication");
    assert!(projected.request.is_none());
}

#[test]
fn recovery_bytes_are_stable_for_fresh_multilabel_maps() -> Result<(), String> {
    let mut expected = None;
    for order in [
        ["runner", "operation", "instance", "revision"],
        ["revision", "instance", "operation", "runner"],
        ["operation", "runner", "revision", "instance"],
    ] {
        let values = [
            ("runner", "velnor".to_owned()),
            ("operation", "execution-publication".to_owned()),
            ("instance", "a".repeat(32)),
            ("revision", "c".repeat(40)),
        ];
        let mut labels = std::collections::HashMap::new();
        for key in order {
            let value = &values
                .iter()
                .find(|(label, _)| *label == key)
                .ok_or_else(|| format!("missing recovery label {key}"))?
                .1;
            labels.insert(key.to_owned(), value.clone());
        }
        let mut value = recovery();
        value.config.labels = Some(labels.clone());
        value.expected_config.labels = Some(labels);
        let options = bollard::query_parameters::CreateContainerOptions {
            name: Some(value.operation_name.clone()),
            platform: value.platform.clone(),
        };
        value.projection_digest = lower_hex(&Sha256::digest(
            crate::launch::resource_probe::projection::canonical::pair(&options, &value.config)
                .expect("projection digest bytes"),
        ));
        let bytes = value.encode().expect("canonical recovery projection");
        if let Some(expected) = &expected {
            assert_eq!(&bytes, expected);
        } else {
            expected = Some(bytes);
        }
    }
    Ok(())
}
