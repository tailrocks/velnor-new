//! Installed container profiles reject ambient or incomplete qualification.
use super::{
    ContainerPlatform, DaemonIdentityPolicy, HostContainerProfile, HostDockerCli, HostDockerDaemon,
    HostOrbStackSdk,
};
use crate::config::{CheckExecutor, CheckPlatform};

mod docker;
mod orbstack;
mod placement;

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
