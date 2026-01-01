use super::super::HostContainerProfile;
use super::{admitted, docker, orb};
use crate::config::{CheckExecutor, CheckPlatform};

#[test]
fn docker_requires_exact_cli_daemon_and_socket_pins() {
    for (field, bad) in [
        ("context", "${{ env.CONTEXT }}"),
        ("socket_path", "unix://host/socket"),
        ("socket_path", "/var/../run/docker.sock"),
    ] {
        let mut value = serde_json::to_value(docker()).expect("fixture");
        value[field] = serde_json::json!(bad);
        let profile: HostContainerProfile = serde_json::from_value(value).expect("shape");
        assert!(!admitted(
            &profile,
            CheckPlatform::LinuxX64,
            CheckExecutor::Hosted
        ));
    }
    for (object, field, bad) in [
        ("cli", "path", "docker"),
        ("cli", "sha256", "latest"),
        ("cli", "version", "latest"),
        ("cli", "build", "$(command)"),
        ("daemon", "operating_system", ""),
        ("daemon", "version", "latest"),
    ] {
        let mut value = serde_json::to_value(docker()).expect("fixture");
        value[object][field] = serde_json::json!(bad);
        let profile: HostContainerProfile = serde_json::from_value(value).expect("shape");
        assert!(!admitted(
            &profile,
            CheckPlatform::LinuxX64,
            CheckExecutor::Hosted
        ));
    }
}
#[test]
fn container_profile_has_no_ambient_or_static_identity_modes() {
    for (object, field, bad) in [
        ("daemon", "identity_policy", "static"),
        ("daemon", "platform", "macos_arm64"),
    ] {
        let mut value = serde_json::to_value(docker()).expect("fixture");
        value[object][field] = serde_json::json!(bad);
        assert!(serde_json::from_value::<HostContainerProfile>(value).is_err());
    }
    for field in ["env", "argv", "daemon_id", "docker_host"] {
        let mut value = serde_json::to_value(docker()).expect("fixture");
        value[field] = serde_json::json!("unqualified");
        assert!(serde_json::from_value::<HostContainerProfile>(value).is_err());
    }
    let mut value = serde_json::to_value(docker()).expect("fixture");
    value["daemon"]
        .as_object_mut()
        .expect("object")
        .remove("identity_policy");
    assert!(serde_json::from_value::<HostContainerProfile>(value).is_err());
}

#[test]
fn docker_socket_owner_is_required_and_root_uid_is_valid() {
    assert_eq!(docker().socket_uid(), 0);
    assert_eq!(orb().socket_uid(), 501);
    assert!(admitted(
        &docker(),
        CheckPlatform::LinuxX64,
        CheckExecutor::Hosted
    ));
    let mut value = serde_json::to_value(docker()).expect("fixture");
    value.as_object_mut().expect("object").remove("socket_uid");
    assert!(serde_json::from_value::<HostContainerProfile>(value).is_err());
    let mut value = serde_json::to_value(docker()).expect("fixture");
    value["socket_uid"] = serde_json::json!(1000);
    let profile: HostContainerProfile = serde_json::from_value(value).expect("explicit UID");
    assert_eq!(profile.socket_uid(), 1000);
}
