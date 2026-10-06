use super::super::HostContainerProfile;
use super::{admitted, orb};
use crate::config::{CheckExecutor, CheckPlatform};

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
