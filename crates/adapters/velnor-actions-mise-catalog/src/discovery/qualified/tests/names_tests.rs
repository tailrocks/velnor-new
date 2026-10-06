use super::super::{ResolvedTools, resolve};
use super::{check, node};
use velnor_actions_contract::config::{
    CheckPlatform, CheckSystemTool, CheckSystemToolKind, ContainerPlatform, DaemonIdentityPolicy,
    HostContainerProfile, HostDockerCli, HostDockerDaemon, HostOrbStackSdk, MiseCheck,
    QualifiedTool, QualifiedToolProbe,
};
use velnor_actions_mise_core::MiseError;

fn named_node(executable: &str, platform: CheckPlatform) -> QualifiedTool {
    let mut tool = node("node", "24.18.0");
    let qualified = &mut tool.platforms[0];
    qualified.platform = platform;
    let os = if platform == CheckPlatform::LinuxX64 {
        "linux"
    } else {
        "darwin"
    };
    qualified.artifacts[0].url = format!(
        "https://nodejs.org/dist/v24.18.0/node-v24.18.0-{os}-{}.tar.xz",
        platform.arch()
    );
    let projected = &mut qualified.executables[0];
    projected.name = executable.to_owned();
    projected.path = format!("bin/{executable}");
    projected.probe = QualifiedToolProbe::Version {
        expected: format!("{executable} 24.18.0"),
    };
    tool
}
fn resolve_named_for_check(
    executable: &str,
    check: &MiseCheck,
) -> Result<ResolvedTools, MiseError> {
    resolve(
        &[named_node(executable, check.runner.platform)],
        &["node".to_owned()],
        check,
    )
}
fn system_tool(kind: CheckSystemToolKind) -> CheckSystemTool {
    CheckSystemTool {
        kind,
        version: "6.2.1".to_owned(),
        build: "swiftlang-6.2.1.1.1".to_owned(),
    }
}
fn docker_profile() -> HostContainerProfile {
    HostContainerProfile::Docker {
        context: "local".to_owned(),
        socket_path: "/run/docker.sock".to_owned(),
        socket_uid: 1000,
        cli: HostDockerCli {
            path: "/usr/bin/docker".to_owned(),
            sha256: "a".repeat(64),
            version: "28.0.0".to_owned(),
            build: "1".to_owned(),
        },
        daemon: HostDockerDaemon {
            version: "28.0.0".to_owned(),
            platform: ContainerPlatform::LinuxX64,
            operating_system: "linux".to_owned(),
            identity_policy: DaemonIdentityPolicy::ExecutionScoped,
        },
    }
}
fn orbstack_profile() -> HostContainerProfile {
    HostContainerProfile::OrbStack {
        context: "local".to_owned(),
        socket_path: "/run/docker.sock".to_owned(),
        cli: HostDockerCli {
            path: "/usr/bin/docker".to_owned(),
            sha256: "a".repeat(64),
            version: "28.0.0".to_owned(),
            build: "1".to_owned(),
        },
        daemon: HostDockerDaemon {
            version: "28.0.0".to_owned(),
            platform: ContainerPlatform::LinuxX64,
            operating_system: "linux".to_owned(),
            identity_policy: DaemonIdentityPolicy::ExecutionScoped,
        },
        sdk: Box::new(HostOrbStackSdk {
            app_bundle_path: "/Applications/OrbStack.app".to_owned(),
            bundle_id: "com.orbstack.Orbstack".to_owned(),
            team_id: "ABCD123456".to_owned(),
            version: "2.0.0".to_owned(),
            build: "1".to_owned(),
            info_plist_sha256: "b".repeat(64),
            main_executable_path: "Contents/MacOS/OrbStack".to_owned(),
            main_executable_sha256: "c".repeat(64),
            cli_bundle_path: "/Applications/OrbStack.app/Contents/cli.app".to_owned(),
            source_tree_sha256: "d".repeat(64),
            owned_tree_sha256: "e".repeat(64),
            cli_relative_path: "Contents/MacOS/orbctl".to_owned(),
            cli_sha256: "f".repeat(64),
            cli_version: "2.0.0".to_owned(),
            cli_build: "1".to_owned(),
            cli_commit: "0".repeat(40),
            runtime_dir: "/Users/test/.orbstack/run".to_owned(),
            runtime_uid: 1000,
        }),
    }
}
#[test]
fn projected_names_reserve_only_runtime_binaries_owned_by_the_check() {
    let linux = check(CheckPlatform::LinuxX64);
    assert!(resolve_named_for_check("mise", &linux).is_err());
    assert!(resolve_named_for_check("docker", &linux).is_ok());
    assert!(resolve_named_for_check("orbctl", &linux).is_ok());

    let mut docker = check(CheckPlatform::LinuxX64);
    docker.runner.container = Some(docker_profile());
    assert!(resolve_named_for_check("docker", &docker).is_err());
    assert!(resolve_named_for_check("orbctl", &docker).is_ok());

    let mut orbstack = check(CheckPlatform::MacosArm64);
    orbstack.runner.container = Some(orbstack_profile());
    assert!(resolve_named_for_check("docker", &orbstack).is_err());
    assert!(resolve_named_for_check("orbctl", &orbstack).is_err());

    let macos = check(CheckPlatform::MacosArm64);
    assert!(resolve_named_for_check("swift", &macos).is_ok());
    assert!(resolve_named_for_check("xcodebuild", &macos).is_ok());

    let mut swift = check(CheckPlatform::MacosArm64);
    swift.system_tools = vec![system_tool(CheckSystemToolKind::Swift)];
    assert!(resolve_named_for_check("swift", &swift).is_err());
    assert!(resolve_named_for_check("xcodebuild", &swift).is_ok());

    let mut xcode = check(CheckPlatform::MacosArm64);
    xcode.system_tools = vec![system_tool(CheckSystemToolKind::Xcode)];
    assert!(resolve_named_for_check("xcodebuild", &xcode).is_err());
    assert!(resolve_named_for_check("swift", &xcode).is_ok());
}
