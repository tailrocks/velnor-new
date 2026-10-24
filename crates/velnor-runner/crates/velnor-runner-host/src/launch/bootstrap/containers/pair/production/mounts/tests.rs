use bollard::models::{
    ContainerInspectResponse, HostConfig, Mount, MountPoint, MountType, MountVolumeOptions,
};

use super::inspect_volume_mounts;

fn expected() -> Mount {
    Mount {
        target: Some("/opt/velnor/source-proof/control".to_owned()),
        source: Some("velnor-test-metadata".to_owned()),
        typ: Some(MountType::VOLUME),
        read_only: Some(true),
        volume_options: Some(MountVolumeOptions {
            no_copy: Some(true),
            subpath: Some("source-proof/control".to_owned()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn inspected(actual: Vec<MountPoint>) -> ContainerInspectResponse {
    ContainerInspectResponse {
        host_config: Some(HostConfig {
            mounts: Some(vec![expected()]),
            tmpfs: Some(std::collections::HashMap::new()),
            ..Default::default()
        }),
        mounts: Some(actual),
        ..Default::default()
    }
}

fn actual(read_write: bool) -> MountPoint {
    MountPoint {
        typ: Some("volume".to_owned()),
        name: Some("velnor-test-metadata".to_owned()),
        destination: Some("/opt/velnor/source-proof/control".to_owned()),
        rw: Some(read_write),
        ..Default::default()
    }
}

#[test]
fn accepts_exact_inspected_subpath_mount() {
    let digest = inspect_volume_mounts(&inspected(vec![actual(false)]), &[expected()], "runner")
        .expect("exact mount");
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
}

#[test]
fn rejects_wrong_access_mode() {
    assert!(
        inspect_volume_mounts(&inspected(vec![actual(true)]), &[expected()], "runner").is_err()
    );
}

#[test]
fn rejects_missing_and_duplicate_mounts() {
    assert!(inspect_volume_mounts(&inspected(Vec::new()), &[expected()], "runner").is_err());
    assert!(
        inspect_volume_mounts(
            &inspected(vec![actual(false), actual(false)]),
            &[expected()],
            "runner",
        )
        .is_err()
    );
}

#[test]
fn rejects_a_different_configured_subpath() {
    let mut response = inspected(vec![actual(false)]);
    let config = response
        .host_config
        .as_mut()
        .and_then(|host| host.mounts.as_mut())
        .expect("mount configuration");
    config[0]
        .volume_options
        .as_mut()
        .expect("volume options")
        .subpath = Some("source-proof/inbox".to_owned());
    assert!(inspect_volume_mounts(&response, &[expected()], "runner").is_err());
}
