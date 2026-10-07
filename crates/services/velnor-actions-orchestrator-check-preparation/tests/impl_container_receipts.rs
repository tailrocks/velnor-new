//! Container receipts bind before/after execution identity.

use serde_json::{Value, json};
use std::path::PathBuf;
use velnor_actions_contract::{canonical_json_bytes, digest_b3};
use velnor_actions_contract_config::config::{
    CheckExecutor, CheckPlatform, CheckRunner, ContainerPlatform, DaemonIdentityPolicy,
    HostContainerProfile, HostDockerCli, HostDockerDaemon,
};
use velnor_actions_mise::checks::{
    CheckCapabilityProof, ContainerObservation, ContainerProbeOutput, DockerDaemonObservation,
};
use velnor_actions_orchestrator_check_preparation::container_receipts::runtime::{
    RuntimeObservation, SocketEvidence,
};
use velnor_actions_orchestrator_check_preparation::container_receipts::{
    ContainerReceipt, container_receipt, validate_container_receipt,
};

fn profile() -> HostContainerProfile {
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

fn runner(container: Option<HostContainerProfile>) -> CheckRunner {
    CheckRunner {
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
    let context_hash = velnor_actions_orchestrator_core::sha256::sha256_hex(b"ci");
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

fn runtime_observation() -> RuntimeObservation {
    RuntimeObservation {
        socket: SocketEvidence {
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

fn receipt() -> ContainerReceipt {
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
fn exact_receipt_validates() {
    let configured = runner(Some(profile()));
    assert!(validate_container_receipt(&configured, Some(&receipt())).is_ok());
}

#[test]
fn profile_digest_drift_rejected() {
    let configured = runner(Some(profile()));
    let mut changed = receipt();
    changed.profile_digest = digest_b3(b"foreign profile");
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());
}

#[test]
fn daemon_identity_change_rejected() {
    let configured = runner(Some(profile()));
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
fn owned_cli_path_escape_rejected() {
    let configured = runner(Some(profile()));
    let mut changed = receipt();
    changed
        .before
        .container
        .as_mut()
        .expect("container")
        .docker_program = PathBuf::from("/usr/bin/docker");
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());
}

#[test]
fn runtime_snapshot_change_rejected() {
    let configured = runner(Some(profile()));
    let mut changed = receipt();
    changed
        .before_runtime
        .as_mut()
        .expect("before runtime")
        .socket
        .inode = 3;
    assert!(validate_container_receipt(&configured, Some(&changed)).is_err());
}

#[test]
fn constructor_validates_and_json_is_strict() {
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
    let mut value = serde_json::to_value(built).expect("json");
    value["foreign"] = json!(true);
    assert!(serde_json::from_value::<ContainerReceipt>(value).is_err());
}
