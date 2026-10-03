//! Installed container profiles reject ambient or incomplete qualification.
use super::{
    ContainerPlatform, DaemonIdentityPolicy, HostContainerProfile, HostDockerCli, HostDockerDaemon,
    HostOrbStackSdk,
};
use crate::config::{CheckExecutor, CheckPlatform};

fn docker() -> HostContainerProfile {
    HostContainerProfile::Docker {
        context: "qualified".to_owned(),
        socket_path: "/var/run/docker.sock".to_owned(),
        socket_uid: 0,
        cli: HostDockerCli {
            path: "/usr/bin/docker".to_owned(),
            sha256: "a".repeat(64),
            version: "29.8.2".to_owned(),
            build: "7fc2dff9bc".to_owned(),
        },
        daemon: HostDockerDaemon {
            version: "29.8.2".to_owned(),
            platform: ContainerPlatform::LinuxX64,
            operating_system: "Ubuntu 26.04".to_owned(),
            identity_policy: DaemonIdentityPolicy::ExecutionScoped,
        },
    }
}
fn orb() -> HostContainerProfile {
    let HostContainerProfile::Docker {
        context,
        cli,
        mut daemon,
        ..
    } = docker()
    else {
        unreachable!("fixture")
    };
    daemon.operating_system = "OrbStack".to_owned();
    HostContainerProfile::OrbStack {
        context,
        cli,
        daemon,
        socket_path: "/Users/ci/.orbstack/run/docker.sock".to_owned(),
        sdk: Box::new(HostOrbStackSdk {
            app_bundle_path: "/Applications/OrbStack.app".to_owned(),
            bundle_id: "dev.kdrag0n.MacVirt".to_owned(),
            team_id: "HUAQ24HBR6".to_owned(),
            version: "2.2.3".to_owned(),
            build: "20963".to_owned(),
            info_plist_sha256: "b".repeat(64),
            main_executable_path: "Contents/MacOS/OrbStack".to_owned(),
            main_executable_sha256: "c".repeat(64),
            cli_bundle_path: "/Applications/OrbStack.app/Contents/MacOS/scli.app".to_owned(),
            source_tree_sha256: "d".repeat(64),
            owned_tree_sha256: "e".repeat(64),
            cli_relative_path: "Contents/MacOS/scli".to_owned(),
            cli_sha256: "f".repeat(64),
            cli_version: "2.2.3".to_owned(),
            cli_build: "2020300".to_owned(),
            cli_commit: "a".repeat(40),
            runtime_dir: "/Users/ci/.orbstack/run".to_owned(),
            runtime_uid: 501,
        }),
    }
}
fn admitted(
    profile: &HostContainerProfile,
    platform: CheckPlatform,
    executor: CheckExecutor,
) -> bool {
    profile
        .validate(platform, executor, "config.toml", "runner.container")
        .is_ok()
}
#[test]
fn installed_container_placement_is_explicit() {
    assert!(admitted(
        &docker(),
        CheckPlatform::LinuxX64,
        CheckExecutor::Hosted
    ));
    assert!(admitted(
        &docker(),
        CheckPlatform::LinuxX64,
        CheckExecutor::EphemeralSelfHosted
    ));
    assert!(admitted(
        &docker(),
        CheckPlatform::MacosArm64,
        CheckExecutor::EphemeralSelfHosted
    ));
    assert!(!admitted(
        &docker(),
        CheckPlatform::MacosArm64,
        CheckExecutor::Hosted
    ));
    assert!(admitted(
        &orb(),
        CheckPlatform::MacosArm64,
        CheckExecutor::EphemeralSelfHosted
    ));
    assert!(!admitted(
        &orb(),
        CheckPlatform::LinuxX64,
        CheckExecutor::EphemeralSelfHosted
    ));
    assert!(!admitted(
        &orb(),
        CheckPlatform::MacosArm64,
        CheckExecutor::Hosted
    ));
}
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
fn orbstack_requires_sdk_tree_and_runtime_authority() {
    for (field, bad) in [
        ("source_tree_sha256", ""),
        ("owned_tree_sha256", ""),
        ("cli_commit", "unknown"),
        ("team_id", "unknown"),
        ("runtime_dir", "/Users/ci"),
        ("cli_bundle_path", "/Applications/Other.app"),
        ("cli_relative_path", "../outside"),
        ("main_executable_path", "Contents/../evil"),
    ] {
        let mut value = serde_json::to_value(orb()).expect("fixture");
        value["sdk"][field] = serde_json::json!(bad);
        let profile: HostContainerProfile = serde_json::from_value(value).expect("shape");
        assert!(
            !admitted(
                &profile,
                CheckPlatform::MacosArm64,
                CheckExecutor::EphemeralSelfHosted
            ),
            "{field}"
        );
    }
    let mut value = serde_json::to_value(orb()).expect("fixture");
    value["sdk"]["runtime_uid"] = serde_json::json!(0);
    let profile: HostContainerProfile = serde_json::from_value(value).expect("shape");
    assert!(!admitted(
        &profile,
        CheckPlatform::MacosArm64,
        CheckExecutor::EphemeralSelfHosted
    ));
    let mut value = serde_json::to_value(orb()).expect("fixture");
    value["socket_path"] = serde_json::json!("/Users/ci/.orbstack/run/nested/docker.sock");
    let profile: HostContainerProfile = serde_json::from_value(value).expect("shape");
    assert!(!admitted(
        &profile,
        CheckPlatform::MacosArm64,
        CheckExecutor::EphemeralSelfHosted
    ));
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
fn hidden_runtime_directory_is_a_path_segment_not_an_identifier() {
    let profile = orb();
    profile
        .validate(
            CheckPlatform::MacosArm64,
            CheckExecutor::EphemeralSelfHosted,
            "config.toml",
            "runner.container",
        )
        .expect("explicit hidden runtime directory");
    for bad in [
        "/Users/ci/../.orbstack/run/docker.sock",
        "/Users/ci//.orbstack/run/docker.sock",
        "/Users/ci/./.orbstack/run/docker.sock",
    ] {
        let mut value = serde_json::to_value(orb()).expect("fixture");
        value["socket_path"] = serde_json::json!(bad);
        let profile: HostContainerProfile = serde_json::from_value(value).expect("shape");
        assert!(!admitted(
            &profile,
            CheckPlatform::MacosArm64,
            CheckExecutor::EphemeralSelfHosted
        ));
    }
}

#[test]
fn orbstack_daemon_operating_system_requires_exact_identity() {
    for operating_system in ["NotOrbStack", "OrbStack Linux", "OrbStack ", "orbstack"] {
        let mut value = serde_json::to_value(orb()).expect("fixture");
        value["daemon"]["operating_system"] = serde_json::json!(operating_system);
        let profile: HostContainerProfile = serde_json::from_value(value).expect("shape");
        assert!(
            !admitted(
                &profile,
                CheckPlatform::MacosArm64,
                CheckExecutor::EphemeralSelfHosted
            ),
            "{operating_system}"
        );
    }
    assert!(admitted(
        &orb(),
        CheckPlatform::MacosArm64,
        CheckExecutor::EphemeralSelfHosted
    ));
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
