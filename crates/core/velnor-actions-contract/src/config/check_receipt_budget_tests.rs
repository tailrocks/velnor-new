use super::*;
use crate::config::{
    CheckExecutor, CheckPlatform, CheckRunner, ContainerPlatform, DaemonIdentityPolicy,
    HostContainerProfile, HostDockerCli, HostDockerDaemon, QualifiedToolArtifact,
    QualifiedToolBackend, QualifiedToolExecutable, QualifiedToolOptions, QualifiedToolProbe,
};

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

#[test]
fn admitted_tool_closure_has_a_truthful_worst_case_receipt_budget() {
    let check = check();
    let mut maximum_admitted = 0;
    for count in 1..=64 {
        let tool = tool(count);
        let bound = check_execution_receipt_upper_bound(&check, std::slice::from_ref(&tool))
            .expect("bounded receipt estimate");
        if bound > MAX_CHECK_EXECUTION_RECEIPT_BYTES {
            break;
        }
        maximum_admitted = count;
        validate_check_budget(&check, &[tool], "config", "checks[0]")
            .expect("the computed fitting closure is admitted");
    }
    assert!(maximum_admitted > 1, "small closures remain usable");
    let allowed = tool(maximum_admitted);
    let bound = check_execution_receipt_upper_bound(&check, std::slice::from_ref(&allowed))
        .expect("max admitted estimate");
    assert!(bound <= MAX_CHECK_EXECUTION_RECEIPT_BYTES);
    let rejected = tool(maximum_admitted + 1);
    let error = validate_check_budget(&check, &[rejected], "config", "checks[0]")
        .expect_err("the next closure cannot fit the report transport");
    assert!(
        error
            .to_string()
            .contains("check_execution_receipt_budget_exceeded")
    );
}

#[test]
fn large_container_profile_reduces_admitted_probes_before_the_receipt_gate() {
    let profile = large_docker_profile();
    profile
        .validate(
            CheckPlatform::LinuxX64,
            CheckExecutor::EphemeralSelfHosted,
            "config",
            "checks[0].runner.container",
        )
        .expect("large profile is within declared bounds");
    let mut check = check();
    check.runner.label = "native-scale".into();
    check.runner.executor = CheckExecutor::EphemeralSelfHosted;
    check.runner.container = Some(profile.clone());
    let mut maximum_admitted = 0;
    for count in 1..=64 {
        let tool = tool(count);
        let bound = check_execution_receipt_upper_bound(&check, std::slice::from_ref(&tool))
            .expect("bounded container receipt estimate");
        if bound > MAX_CHECK_EXECUTION_RECEIPT_BYTES {
            break;
        }
        maximum_admitted = count;
        validate_check_budget(&check, &[tool], "config", "checks[0]")
            .expect("fitting container closure is admitted");
    }
    assert!(
        maximum_admitted > 0,
        "container checks retain a usable closure"
    );
    let allowed = tool(maximum_admitted);
    assert!(
        check_execution_receipt_upper_bound(&check, std::slice::from_ref(&allowed))
            .expect("max admitted bound")
            <= MAX_CHECK_EXECUTION_RECEIPT_BYTES
    );
    let rejected = tool(maximum_admitted + 1);
    assert!(
        validate_check_budget(&check, &[rejected], "config", "checks[0]")
            .expect_err("the next container closure exceeds the staged proof limit")
            .to_string()
            .contains("check_execution_receipt_budget_exceeded")
    );
}
