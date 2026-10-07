use super::*;
use serde_json::json;
use std::path::PathBuf;
use velnor_actions_contract::{digest_b3, is_valid_digest};
use velnor_actions_contract_config::config::{
    ContainerPlatform, DaemonIdentityPolicy, HostContainerProfile, HostDockerCli, HostDockerDaemon,
};
use velnor_actions_mise::checks::CheckCapabilityProof;

mod container_tests;
mod generators;

use generators::*;

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
