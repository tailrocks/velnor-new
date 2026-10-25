use super::{PublicationProjection, digest, request as make_request};
use crate::launch::resource_probe::projection::VerifiedProbeImage;
use velnor_resource_probe::{
    execution_channel_wire::ExecutionChannelDirectory,
    execution_publication::{ExecutionPublicationContext, ExecutionPublicationKind},
};

fn control() -> ExecutionChannelDirectory {
    ExecutionChannelDirectory {
        target: "/opt/velnor/source-proof/control".to_owned(),
        inode: 123,
        owner_uid: 0,
        owner_gid: 0,
        mode: 0o555,
        mount_id: 12,
        statx_mount_id: 12,
        mount_root: "/source-proof/control".to_owned(),
        mount_source: "/dev/owned".to_owned(),
        mount_device_major: 0,
        mount_device_minor: 44,
        statx_device_major: 0,
        statx_device_minor: 44,
        filesystem_type_magic: 0xef53,
        filesystem_type_name: "ext4".to_owned(),
        filesystem_id: 45,
        read_only: true,
        executable_mount: false,
    }
}

fn request() -> ExecutionPublicationContext {
    let payload = br#"{"challenge":"c"}"#;
    make_request(
        "0123456789abcdef0123456789abcdef".to_owned(),
        "123e4567-e89b-12d3-a456-426614174000",
        ExecutionPublicationKind::Challenge,
        payload,
        &digest(payload),
        &control(),
    )
    .expect("valid request")
}

#[test]
fn publisher_projection_is_narrow_and_has_one_owned_rw_volume() {
    let request = request();
    let projection = PublicationProjection::build(
        request.operation_id.clone(),
        &"1".repeat(32),
        "selected-engine",
        "/var/lib/docker",
        "metadata-owned",
        &VerifiedProbeImage::test_fixture(),
        request,
    )
    .expect("valid projection");
    let host = projection.config.host_config.expect("host config");
    assert_eq!(host.privileged, Some(false));
    assert_eq!(host.readonly_rootfs, Some(true));
    assert_eq!(host.network_mode.as_deref(), Some("none"));
    assert_eq!(host.cap_drop.as_deref(), Some(&["ALL".to_owned()][..]));
    assert_eq!(
        host.cap_add.as_deref(),
        Some(&["DAC_OVERRIDE".to_owned()][..])
    );
    let mounts = host.mounts.expect("one volume mount");
    assert_eq!(mounts.len(), 1);
    assert_eq!(mounts[0].source.as_deref(), Some("metadata-owned"));
    assert_eq!(mounts[0].target.as_deref(), Some("/opt/velnor"));
    assert_eq!(mounts[0].read_only, Some(false));
    assert_eq!(
        mounts[0]
            .volume_options
            .as_ref()
            .and_then(|value| value.no_copy),
        Some(true)
    );
}

#[test]
fn production_projection_digest_is_stable_for_labeled_hash_maps() {
    let build = || {
        let request = request();
        PublicationProjection::build(
            request.operation_id.clone(),
            &"1".repeat(32),
            "selected-engine",
            "/var/lib/docker",
            "metadata-owned",
            &VerifiedProbeImage::test_fixture(),
            request,
        )
        .expect("valid projection")
    };
    let first = build();
    let second = build();
    let options = bollard::query_parameters::CreateContainerOptions {
        name: Some(first.name.clone()),
        platform: "linux/amd64".to_owned(),
    };
    let expected = digest(
        &crate::launch::resource_probe::projection::canonical::pair(&options, &first.config)
            .expect("canonical projection bytes"),
    );

    assert_eq!(first.digest, expected);
    assert_eq!(first.digest, second.digest);
    assert!(
        first
            .config
            .labels
            .as_ref()
            .is_some_and(|labels| labels.len() >= 5)
    );
}

#[test]
fn production_projection_roundtrips_through_canonical_recovery_bytes() {
    let mut expected_bytes = None;
    for _ in 0..8 {
        let request = request();
        let payload_sha256 = request.payload_sha256.clone();
        let projection = PublicationProjection::build(
            request.operation_id.clone(),
            &"1".repeat(32),
            "selected-engine",
            "/var/lib/docker",
            "metadata-owned",
            &VerifiedProbeImage::test_fixture(),
            request,
        )
        .expect("valid production projection");
        assert!(
            projection
                .config
                .labels
                .as_ref()
                .is_some_and(|labels| labels.len() >= 5)
        );
        let parent = crate::journal::ExecutionPublicationParent::test_fixture(
            &projection.engine_id,
            projection.root.digest(),
            &projection.metadata_volume,
            &payload_sha256,
        );
        let recovery = super::recovery::PublicationRecoveryProjection::from_live(
            &projection,
            "metadata-owned",
            "/var/lib/docker/volumes/metadata-owned/_data",
            &parent,
        )
        .expect("recovery projection from production state");
        let bytes = recovery.encode().expect("canonical recovery bytes");
        let digest = digest(bytes.as_bytes());
        let decoded = super::recovery::PublicationRecoveryProjection::decode(&bytes, &digest)
            .expect("canonical recovery decode");
        let restored = decoded
            .into_projection("/var/lib/docker")
            .expect("valid restored projection");
        assert_eq!(restored.digest, projection.digest);
        assert_eq!(restored.config.labels, projection.config.labels);
        assert_eq!(
            crate::launch::resource_probe::projection::canonical::pair(
                &restored.options,
                &restored.config,
            )
            .expect("canonical restored projection bytes"),
            crate::launch::resource_probe::projection::canonical::pair(
                &projection.options,
                &projection.config,
            )
            .expect("canonical production projection bytes"),
        );
        if let Some(previous) = expected_bytes.as_ref() {
            assert_eq!(previous, &bytes);
        } else {
            expected_bytes = Some(bytes);
        }
    }
}

#[test]
fn effective_container_config_keeps_verified_image_defaults_and_overrides() {
    let request = request();
    let projection = PublicationProjection::build(
        request.operation_id.clone(),
        &"1".repeat(32),
        "selected-engine",
        "/var/lib/docker",
        "metadata-owned",
        &VerifiedProbeImage::test_fixture(),
        request,
    )
    .expect("valid projection");
    assert_eq!(
        projection.expected_config.env,
        Some(vec![
            "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned()
        ])
    );
    assert_eq!(projection.expected_config.user.as_deref(), Some("0:0"));
    assert_eq!(
        projection.expected_config.entrypoint.as_deref(),
        Some(["/velnor/resource-probe".to_owned()].as_slice())
    );
    let labels = projection.expected_config.labels.expect("effective labels");
    assert_eq!(
        labels
            .get("org.opencontainers.image.revision")
            .map(String::as_str),
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
    assert_eq!(
        labels.get("velnor.role").map(String::as_str),
        Some("execution-publication")
    );
}

#[test]
fn publisher_request_rejects_changed_payload_and_channel_target() {
    let payload = br#"{"challenge":"c"}"#;
    assert!(
        make_request(
            "0123456789abcdef0123456789abcdef".to_owned(),
            "123e4567-e89b-12d3-a456-426614174000",
            ExecutionPublicationKind::Challenge,
            payload,
            &"f".repeat(64),
            &control(),
        )
        .is_err()
    );
    let mut wrong = control();
    wrong.target = "/tmp/control".to_owned();
    let value = ExecutionPublicationContext {
        expected_runner_control: wrong,
        ..request()
    };
    assert!(
        PublicationProjection::build(
            value.operation_id.clone(),
            &"1".repeat(32),
            "selected-engine",
            "/var/lib/docker",
            "metadata-owned",
            &VerifiedProbeImage::test_fixture(),
            value,
        )
        .is_err()
    );
}
