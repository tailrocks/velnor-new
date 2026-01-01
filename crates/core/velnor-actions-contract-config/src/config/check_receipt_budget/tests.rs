use super::*;
use crate::config::{
    CheckExecutor, CheckPlatform, CheckRunner, ContainerPlatform, DaemonIdentityPolicy,
    HostContainerProfile, HostDockerCli, HostDockerDaemon, QualifiedToolArtifact,
    QualifiedToolBackend, QualifiedToolExecutable, QualifiedToolOptions, QualifiedToolProbe,
};

mod closure;
mod container;

fn check() -> MiseCheck {
    MiseCheck {
        id: "bounded".into(),
        task: "test".into(),
        directory: ".".into(),
        runner: CheckRunner {
            label: "ubuntu-24.04".into(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec!["Cargo.toml".into()],
        tools: vec!["bun".into()],
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 30,
    }
}

fn tool(executable_count: usize) -> QualifiedTool {
    let executables = (0..executable_count)
        .map(|index| QualifiedToolExecutable {
            name: format!("bun-{index:03}"),
            path: format!("bin/bun-{index:03}"),
            sha256: "c".repeat(64),
            probe: QualifiedToolProbe::Version {
                expected: "bun 1.3.14".into(),
            },
        })
        .collect();
    QualifiedTool {
        id: "bun".into(),
        backend: QualifiedToolBackend::Core { tool: "bun".into() },
        version: "1.3.14".into(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifact {
                url:
                    "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun-linux-x64.zip"
                        .into(),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: Vec::new(),
            install_tree_sha256: "b".repeat(64),
            executables,
        }],
    }
}

fn large_docker_profile() -> HostContainerProfile {
    let component = "a".repeat(240);
    let path = |leaf: &str| format!("/{component}/{component}/{component}/{component}/{leaf}");
    HostContainerProfile::Docker {
        context: "c".repeat(128),
        socket_path: path("docker.sock"),
        socket_uid: 0,
        cli: HostDockerCli {
            path: path("docker"),
            sha256: "a".repeat(64),
            version: "1.2.3".into(),
            build: "b".repeat(128),
        },
        daemon: HostDockerDaemon {
            version: "1.2.3".into(),
            platform: ContainerPlatform::LinuxX64,
            operating_system: "D".repeat(256),
            identity_policy: DaemonIdentityPolicy::ExecutionScoped,
        },
    }
}
