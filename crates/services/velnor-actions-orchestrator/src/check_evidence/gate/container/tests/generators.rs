use super::super::runtime;
use super::profile;
use serde_json::{Value, json};
use std::path::PathBuf;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, ContainerPlatform, HostContainerProfile,
};
use velnor_actions_contract::digest_b3;
use velnor_actions_mise::checks::{
    ContainerObservation, ContainerProbeOutput, DockerDaemonObservation,
};

pub(super) fn orb_profile() -> HostContainerProfile {
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

pub(super) fn runner(
    container: Option<HostContainerProfile>,
) -> velnor_actions_contract::CheckRunner {
    velnor_actions_contract::CheckRunner {
        label: "native-scale".into(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container,
    }
}

pub(super) fn probe(stdout: &str) -> ContainerProbeOutput {
    ContainerProbeOutput {
        stdout: stdout.into(),
        stderr: String::new(),
        stdout_digest: digest_b3(stdout.as_bytes()),
        stderr_digest: digest_b3(b""),
    }
}

pub(super) fn observation() -> ContainerObservation {
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

pub(super) fn runtime() -> Value {
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

pub(super) fn runtime_observation() -> runtime::RuntimeObservation {
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

pub(super) fn orb_observation() -> ContainerObservation {
    let mut observed = observation();
    observed.profile = orb_profile();
    observed.endpoint = "unix:///Users/test/.orbstack/run/docker.sock".into();
    observed.daemon.operating_system = "OrbStack".into();
    observed
}

pub(super) fn orb_runtime() -> (Value, runtime::RuntimeObservation) {
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
