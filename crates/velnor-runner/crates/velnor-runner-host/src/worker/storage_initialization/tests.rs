use bollard::models::MountType;

use super::{request_result, specification as spec};
use crate::{HostError, worker::test_resource_budget};

const VALID: &str = r#"{"schema":1,"operation_id":"op-7","home":{"target":"/velnor/init/home","mount_id":31,"parent_mount_id":1,"device_major":0,"device_minor":48,"inode":61,"uid":1000,"gid":1000,"mode":493},"work":{"target":"/velnor/init/work","mount_id":32,"parent_mount_id":1,"device_major":0,"device_minor":48,"inode":62,"uid":1000,"gid":1000,"mode":493}}"#;

fn output(record: &str) -> Vec<u8> {
    format!("{record}\n").into_bytes()
}

fn observed(record: &str) -> Result<request_result::ObservedStorageInitialization, HostError> {
    request_result::parse_result(Some(0), "op-7", &output(record))
}

fn limits() -> Result<spec::StorageInitializationLimits, HostError> {
    spec::StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )
}

#[test]
fn accepts_the_utility_success_record() -> Result<(), HostError> {
    let result = observed(VALID)?;
    assert_eq!(result.operation_id(), "op-7");
    assert_eq!(result.home().mount_id(), 31);
    assert_eq!(result.work().mount_id(), 32);
    assert_eq!(result.home().parent_mount_id(), 1);
    assert_eq!(result.work().device(), (0, 48));
    assert_eq!(result.home().inode(), 61);
    Ok(())
}

#[test]
fn request_is_bounded_and_has_no_extra_input() -> Result<(), HostError> {
    let request = request_result::StorageInitializationRequest::new("op-7")?;
    assert_eq!(
        request.stdin_payload()?,
        br#"{"operation_id":"op-7"}"#.to_vec()
    );
    assert!(request_result::StorageInitializationRequest::new("").is_err());
    assert!(request_result::StorageInitializationRequest::new("../foreign").is_err());
    assert!(request_result::StorageInitializationRequest::new(&"a".repeat(65)).is_err());
    Ok(())
}

#[test]
fn rejects_nonzero_exit_and_missing_exit_status() {
    assert!(request_result::parse_result(Some(1), "op-7", &output(VALID)).is_err());
    assert!(request_result::parse_result(None, "op-7", &output(VALID)).is_err());
}

#[test]
fn rejects_partial_or_unbounded_output() {
    assert!(request_result::parse_result(Some(0), "op-7", b"{\"schema\":1").is_err());
    assert!(request_result::parse_result(Some(0), "op-7", &vec![b'x'; 2_049]).is_err());
    assert!(request_result::parse_result(Some(0), "op-7", VALID.as_bytes()).is_err());
    let mut trailing = output(VALID);
    trailing.extend_from_slice(b"{}\n");
    assert!(request_result::parse_result(Some(0), "op-7", &trailing).is_err());
    let mut trailing_space = VALID.as_bytes().to_vec();
    trailing_space.extend_from_slice(b" \n");
    assert!(request_result::parse_result(Some(0), "op-7", &trailing_space).is_err());
}

#[test]
fn rejects_unknown_fields_and_failed_utility_records() {
    let unknown = VALID.replace("\"schema\":1", "\"schema\":1,\"extra\":true");
    assert!(observed(&unknown).is_err());
    let duplicate = VALID.replace("\"schema\":1", "\"schema\":1,\"schema\":1");
    assert!(observed(&duplicate).is_err());
    let failure = r#"{"schema":1,"operation_id":"op-7","status":"failed","error":"target_owner"}"#;
    assert!(observed(failure).is_err());
}

#[test]
fn rejects_wrong_operation_and_schema() {
    assert!(request_result::parse_result(Some(0), "op-8", &output(VALID)).is_err());
    assert!(observed(&VALID.replace("\"schema\":1", "\"schema\":2")).is_err());
}

#[test]
fn rejects_wrong_targets_owner_group_and_mode() {
    for (from, to) in [
        ("/velnor/init/home", "/foreign/home"),
        ("\"uid\":1000", "\"uid\":0"),
        ("\"gid\":1000", "\"gid\":0"),
        ("\"mode\":493", "\"mode\":511"),
    ] {
        assert!(observed(&VALID.replacen(from, to, 1)).is_err());
    }
}

#[test]
fn rejects_invalid_mount_and_root_identity() {
    for (from, to) in [
        ("\"mount_id\":31", "\"mount_id\":0"),
        ("\"parent_mount_id\":1", "\"parent_mount_id\":0"),
        ("\"mount_id\":31", "\"mount_id\":1"),
        ("\"mount_id\":32", "\"mount_id\":31"),
        ("\"inode\":62", "\"inode\":61"),
    ] {
        assert!(observed(&VALID.replacen(from, to, 1)).is_err());
    }
}

#[test]
fn initializer_limits_must_fit_the_runner_reservation() -> Result<(), HostError> {
    assert!(
        spec::StorageInitializationLimits::from_reserved_budget(
            test_resource_budget()?,
            0,
            128 * 1024 * 1024,
            16
        )
        .is_err()
    );
    assert!(
        spec::StorageInitializationLimits::from_reserved_budget(
            test_resource_budget()?,
            1_001,
            128 * 1024 * 1024,
            16
        )
        .is_err()
    );
    assert!(
        spec::StorageInitializationLimits::from_reserved_budget(
            test_resource_budget()?,
            1,
            2_147_483_649,
            1
        )
        .is_err()
    );
    assert!(
        spec::StorageInitializationLimits::from_reserved_budget(
            test_resource_budget()?,
            1,
            128 * 1024 * 1024,
            0
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn initializer_rejects_memory_below_docker_minimum() -> Result<(), HostError> {
    assert!(
        spec::StorageInitializationLimits::from_reserved_budget(
            test_resource_budget()?,
            1,
            6_291_455,
            1
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn specification_has_only_two_owned_nocopy_volumes_and_no_host_access() -> Result<(), HostError> {
    let create = spec::specification(
        "op-7",
        "home-volume",
        "work-volume",
        &spec::test_image(),
        limits()?,
    )?;
    assert_eq!(create.options.name.as_deref(), Some("velnor-init-op-7"));
    assert_eq!(create.options.platform, "linux/amd64");
    assert_eq!(create.config.user.as_deref(), Some("0:0"));
    assert_eq!(
        create.config.entrypoint.as_deref(),
        Some(["/velnor/storage-init".to_owned()].as_slice())
    );
    assert!(create.config.env.is_none());
    assert!(create.config.cmd.is_none());
    assert_eq!(create.config.network_disabled, Some(true));
    let host = create.config.host_config.ok_or(HostError::Docker)?;
    assert_eq!(host.network_mode.as_deref(), Some("none"));
    assert_eq!(host.privileged, Some(false));
    assert_eq!(host.readonly_rootfs, Some(true));
    assert_eq!(
        host.cap_drop.as_deref(),
        Some(["ALL".to_owned()].as_slice())
    );
    assert_eq!(
        host.cap_add.as_deref(),
        Some(["CHOWN".to_owned(), "FOWNER".to_owned()].as_slice())
    );
    assert_eq!(host.pids_limit, Some(16));
    let mounts = host.mounts.ok_or(HostError::Docker)?;
    assert_eq!(mounts.len(), 2);
    let targets = ["/velnor/init/home", "/velnor/init/work"];
    for (mount, (source, target)) in mounts
        .iter()
        .zip(["home-volume", "work-volume"].into_iter().zip(targets))
    {
        assert_eq!(mount.typ, Some(MountType::VOLUME));
        assert_eq!(mount.source.as_deref(), Some(source));
        assert_eq!(mount.target.as_deref(), Some(target));
        assert_eq!(mount.read_only, Some(false));
        assert_eq!(
            mount
                .volume_options
                .as_ref()
                .and_then(|value| value.no_copy),
            Some(true)
        );
        assert!(mount.bind_options.is_none());
    }
    Ok(())
}

#[test]
fn specification_rejects_duplicate_or_injected_volume_names() -> Result<(), HostError> {
    assert!(spec::specification("op-7", "same", "same", &spec::test_image(), limits()?).is_err());
    assert!(
        spec::specification("op-7", "../foreign", "work", &spec::test_image(), limits()?).is_err()
    );
    assert!(
        spec::specification("op-7", "home", "/host/path", &spec::test_image(), limits()?).is_err()
    );
    Ok(())
}

#[test]
fn specification_uses_the_verified_amd64_runner_image() -> Result<(), HostError> {
    let images = crate::launch::verified_images::test_worker_images();
    let image = spec::StorageInitializationImage::from_verified_release(&images)?;
    assert_eq!(image.source_sha(), images.source_sha());
    let create = spec::specification("op-7", "home-volume", "work-volume", &image, limits()?)?;
    assert_eq!(
        create.config.image.as_deref(),
        Some(images.runner().runtime_id())
    );

    let wrong_platform = crate::launch::verified_images::test_worker_images_with(Some(
        crate::launch::verified_images::TestWorkerImageMutation::Platform,
    ));
    assert!(spec::StorageInitializationImage::from_verified_release(&wrong_platform).is_err());
    Ok(())
}
