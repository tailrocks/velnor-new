use super::*;
use serde_json::json;
use std::path::PathBuf;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, ContainerPlatform, DaemonIdentityPolicy, HostContainerProfile,
    HostDockerCli, HostDockerDaemon,
};
use velnor_actions_contract::{digest_b3, is_valid_digest};
use velnor_actions_mise::checks::{
    CheckCapabilityProof, ContainerObservation, ContainerProbeOutput, DockerDaemonObservation,
};

pub(super) fn profile() -> HostContainerProfile {
    HostContainerProfile::Docker {
        context: "ci".into(),
        socket_path: "/run/docker.sock".into(),
        socket_uid: 0,
        cli: HostDockerCli {
            path: "/usr/local/bin/docker".into(),
            sha256: "a".repeat(64),
            version: "1.2.3".into(),
            build: "build".into(),
        },
        daemon: HostDockerDaemon {
            version: "1.2.3".into(),
            platform: ContainerPlatform::LinuxX64,
            operating_system: "Docker".into(),
            identity_policy: DaemonIdentityPolicy::ExecutionScoped,
        },
    }
}

fn orb_profile() -> HostContainerProfile {
    let mut value = serde_json::to_value(profile()).expect("profile");
    value["provider"] = json!("orb_stack");
    value.as_object_mut().expect("object").remove("socket_uid");
    value["socket_path"] = json!("/Users/test/.orbstack/run/docker.sock");
    value["daemon"]["operating_system"] = json!("OrbStack");
    value["sdk"] = json!({
        "app_bundle_path":"/Applications/OrbStack.app",
        "bundle_id":"com.orbstack.OrbStack",
        "team_id":"TEAMID1234",
        "version":"1.2.3",
        "build":"123",
        "info_plist_sha256":"b".repeat(64),
        "main_executable_path":"Contents/MacOS/OrbStack",
        "main_executable_sha256":"c".repeat(64),
        "cli_bundle_path":"/Applications/OrbStack.app/Contents/CLI.app",
        "source_tree_sha256":"d".repeat(64),
        "owned_tree_sha256":"e".repeat(64),
        "cli_relative_path":"Contents/MacOS/orbctl",
        "cli_sha256":"f".repeat(64),
        "cli_version":"1.2.3",
        "cli_build":"123",
        "cli_commit":"a".repeat(40),
        "runtime_dir":"/Users/test/.orbstack/run",
        "runtime_uid":501
    });
    serde_json::from_value(value).expect("orbstack profile")
}

fn runner(container: Option<HostContainerProfile>) -> velnor_actions_contract::CheckRunner {
    velnor_actions_contract::CheckRunner {
        label: "native-scale".into(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container,
    }
}

fn probe(stdout: &str) -> ContainerProbeOutput {
    ContainerProbeOutput {
        stdout: stdout.into(),
        stderr: String::new(),
        stdout_digest: digest_b3(stdout.as_bytes()),
        stderr_digest: digest_b3(b""),
    }
}

fn observation() -> ContainerObservation {
    let endpoint = "unix:///run/docker.sock";
    let daemon_probe = r#"{"ID":"daemon-1","ServerVersion":"1.2.3","OSType":"linux","Architecture":"x86_64","OperatingSystem":"Docker"}"#;
    ContainerObservation {
        profile: profile(),
        docker_program: PathBuf::from("/tmp/velnor-check/bin/docker"),
        docker_sha256: "a".repeat(64),
        endpoint: endpoint.into(),
        docker_cli: probe("Docker version 1.2.3, build build"),
        context_probe: probe(endpoint),
        daemon: DockerDaemonObservation {
            id: "daemon-1".into(),
            version: "1.2.3".into(),
            platform: ContainerPlatform::LinuxX64,
            architecture: "x86_64".into(),
            operating_system: "Docker".into(),
            probe: probe(daemon_probe),
        },
        orbctl: None,
    }
}

fn runtime() -> Value {
    let context_hash = crate::cover_identity::generator::sha256_hex(b"ci");
    json!({
        "endpoint":"unix:///run/docker.sock",
        "context":"ci",
        "docker_config":"/tmp/velnor-check/docker",
        "context_metadata":format!("/tmp/velnor-check/docker/contexts/meta/{context_hash}/meta.json"),
        "context_hash":context_hash,
        "runtime_dir":null,
        "runtime_link":null,
        "runtime_entries":[{"path":"/run/docker.sock","kind":"unix_socket","owner":0}],
        "socket": {
            "path":"/run/docker.sock",
            "owner":0,
            "group":0,
            "mode":0o140_600,
            "device":1,
            "inode":2
        },
        "runtime_root":null
    })
}

fn runtime_observation() -> runtime::RuntimeObservation {
    runtime::RuntimeObservation {
        socket: runtime::SocketEvidence {
            path: "/run/docker.sock".into(),
            owner: 0,
            group: 0,
            mode: 0o140_600,
            device: 1,
            inode: 2,
        },
        runtime_root: None,
    }
}

fn orb_observation() -> ContainerObservation {
    let mut observed = observation();
    observed.profile = orb_profile();
    observed.endpoint = "unix:///Users/test/.orbstack/run/docker.sock".into();
    observed.daemon.operating_system = "OrbStack".into();
    observed
}

fn orb_runtime() -> (Value, runtime::RuntimeObservation) {
    let mut value = runtime();
    value["endpoint"] = json!("unix:///Users/test/.orbstack/run/docker.sock");
    value["runtime_dir"] = json!("/Users/test/.orbstack/run");
    value["runtime_link"] = json!("/tmp/velnor-check/.orbstack/run");
    value["runtime_entries"] = json!([
        {"path":"docker.sock","kind":"unix_socket","owner":501}
    ]);
    value["socket"] = json!({
        "path":"/Users/test/.orbstack/run/docker.sock",
        "owner":501,
        "group":0,
        "mode":0o140_600,
        "device":1,
        "inode":2
    });
    value["runtime_root"] = json!({
        "path":"/Users/test/.orbstack/run",
        "owner":501,
        "group":0,
        "mode":0o040_755,
        "device":1,
        "inode":3
    });
    let observation = runtime::RuntimeObservation {
        socket: runtime::SocketEvidence {
            path: "/Users/test/.orbstack/run/docker.sock".into(),
            owner: 501,
            group: 0,
            mode: 0o140_600,
            device: 1,
            inode: 2,
        },
        runtime_root: Some(runtime::RuntimeRootEvidence {
            path: "/Users/test/.orbstack/run".into(),
            owner: 501,
            group: 0,
            mode: 0o040_755,
            device: 1,
            inode: 3,
        }),
    };
    (value, observation)
}

pub(super) fn receipt() -> ContainerReceipt {
    let proof = CheckCapabilityProof {
        container: Some(observation()),
    };
    let profile = profile();
    ContainerReceipt {
        profile_digest: digest_b3(&canonical_json_bytes(&profile).expect("profile")),
        before: proof.clone(),
        after: proof,
        sdk: None,
        runtime: runtime(),
        before_runtime: Some(runtime_observation()),
        after_runtime: Some(runtime_observation()),
    }
}

#[test]
fn container_presence_is_mandatory_and_non_container_is_empty() {
    assert!(validate_container_receipt(&runner(None), None).is_ok());
    assert!(validate_container_receipt(&runner(Some(profile())), None).is_err());
    assert!(validate_container_receipt(&runner(None), Some(&receipt())).is_err());
}

#[test]
fn exact_profile_and_execution_identity_are_bound() {
    let configured = runner(Some(profile()));
    let mut valid = receipt();
    assert!(validate_container_receipt(&configured, Some(&valid)).is_ok());
    valid.profile_digest = digest_b3(b"foreign profile");
    assert!(validate_container_receipt(&configured, Some(&valid)).is_err());
    let mut changed = receipt();
    changed
        .after
        .container
        .as_mut()
        .expect("container")
        .daemon
        .id = "daemon-2".into();
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());
}

#[test]
fn owned_cli_and_runtime_inventory_are_exact() {
    let configured = runner(Some(profile()));
    for mutation in 0..4 {
        let mut changed = receipt();
        match mutation {
            0 => {
                changed
                    .before
                    .container
                    .as_mut()
                    .expect("container")
                    .docker_program = PathBuf::from("/usr/bin/docker");
            }
            1 => {
                changed
                    .before
                    .container
                    .as_mut()
                    .expect("container")
                    .docker_sha256 = "b".repeat(64);
            }
            2 => changed.runtime["context_hash"] = json!("foreign"),
            _ => changed.runtime["runtime_entries"][0]["owner"] = json!(1),
        }
        assert!(
            validate_container_receipt(&configured, Some(&changed)).is_err(),
            "{mutation}"
        );
    }
}

#[test]
fn constructor_validates_before_returning_and_receipt_is_strict_json() {
    let configured = runner(Some(profile()));
    let before = CheckCapabilityProof {
        container: Some(observation()),
    };
    let after = before.clone();
    let built = container_receipt(
        &configured,
        before,
        after,
        Option::<Value>::None,
        Some(runtime()),
        Some(runtime_observation()),
        Some(runtime_observation()),
    )
    .expect("receipt")
    .expect("container");
    assert!(is_valid_digest(&built.profile_digest));
    let mut value = serde_json::to_value(built).expect("json");
    value["foreign"] = json!(true);
    assert!(serde_json::from_value::<ContainerReceipt>(value).is_err());
}

#[test]
fn orbstack_requires_the_sdk_projection_evidence() {
    let profile = orb_profile();
    assert!(validate_sdk(&profile, None, &observation(), &observation()).is_err());
}

#[test]
fn runtime_snapshot_and_root_shape_are_bound() {
    let configured = runner(Some(profile()));
    let mut changed = receipt();
    changed
        .before_runtime
        .as_mut()
        .expect("before runtime")
        .socket
        .inode = 3;
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());

    let mut changed = receipt();
    changed.runtime["socket"]["device"] = json!(99);
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());

    let mut changed = receipt();
    changed.runtime["runtime_root"] = json!({
        "path":"/tmp/runtime",
        "owner":0,
        "group":0,
        "mode":0o040_700,
        "device":1,
        "inode":3
    });
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());

    let mut changed = receipt();
    changed
        .runtime
        .as_object_mut()
        .expect("runtime")
        .remove("socket");
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());
}

#[test]
fn orbstack_runtime_root_is_required_and_bound() {
    let profile = orb_profile();
    let observed = orb_observation();
    let (mut value, runtime_observation) = orb_runtime();
    assert!(
        runtime::validate(
            &profile,
            &value,
            &observed,
            &observed,
            &runtime_observation,
            &runtime_observation,
        )
        .is_ok()
    );

    value["runtime_root"] = json!(null);
    assert!(
        runtime::validate(
            &profile,
            &value,
            &observed,
            &observed,
            &runtime_observation,
            &runtime_observation,
        )
        .is_err()
    );

    let (mut value, runtime_observation) = orb_runtime();
    value["runtime_root"]["inode"] = json!(4);
    assert!(
        runtime::validate(
            &profile,
            &value,
            &observed,
            &observed,
            &runtime_observation,
            &runtime_observation,
        )
        .is_err()
    );
}
