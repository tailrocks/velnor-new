use super::*;

use crate::check_evidence::gate::tools::{QualifiedToolReceipt, receipt};
use std::path::PathBuf;
use velnor_actions_contract::config::{
    CheckExecutor, CheckRunner, HostContainerProfile, MAX_CHECK_CONTAINER_PATH_BYTES,
    MAX_CHECK_EXECUTION_RECEIPT_BYTES, MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES, MiseCheck,
    QualifiedToolArtifact, QualifiedToolBackend, QualifiedToolExecutable, QualifiedToolOptions,
    QualifiedToolPlatform, QualifiedToolProbe,
};
use velnor_actions_mise::check_tool_probes::{
    QualifiedExecutableObservation, QualifiedExecutableProof,
};

fn declaration(id: String) -> QualifiedTool {
    let mut expected = "bun 1.3.14".to_owned();
    while expected.len() + 2 <= MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES {
        expected.push('"');
        expected.push('\\');
    }
    if expected.len() < MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES {
        expected.push('x');
    }
    let package = format!("owner/{id}");
    QualifiedTool {
        id,
        backend: QualifiedToolBackend::Aqua {
            package: package.clone(),
        },
        version: "1.3.14".into(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifact {
                url: format!("https://github.com/{package}/releases/download/v1.3.14/tool.zip"),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: Vec::new(),
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: "bun".into(),
                path: "bin/bun".into(),
                sha256: "c".repeat(64),
                probe: QualifiedToolProbe::Version { expected },
            }],
        }],
    }
}

fn selected_check(declarations: &[QualifiedTool]) -> MiseCheck {
    MiseCheck {
        id: "demo".into(),
        task: "test".into(),
        directory: ".".into(),
        runner: CheckRunner {
            label: "ubuntu-24.04".into(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec!["Cargo.toml".into()],
        tools: declarations.iter().map(|tool| tool.id.clone()).collect(),
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 30,
    }
}

fn selected_container_check(
    declarations: &[QualifiedTool],
    profile: HostContainerProfile,
) -> MiseCheck {
    let mut check = selected_check(declarations);
    check.runner = CheckRunner {
        label: "native-scale".into(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container: Some(profile),
    };
    check
}

fn proof(tool: &QualifiedTool) -> QualifiedToolReceipt {
    let platform = &tool.platforms[0];
    let declaration = &platform.executables[0];
    let expected = match &declaration.probe {
        QualifiedToolProbe::Version { expected } => expected.clone(),
        _ => panic!("fixture uses a version probe"),
    };
    let suffix = format!("/tools/{}/prefix/{}", tool.id, declaration.path);
    let target = MAX_CHECK_CONTAINER_PATH_BYTES;
    let mut observed_path = "/".to_owned();
    let mut remaining = target - suffix.len() - 1;
    while remaining > 200 {
        observed_path.push_str(&"a".repeat(200));
        observed_path.push('/');
        remaining -= 201;
    }
    observed_path.push_str(&"a".repeat(remaining));
    observed_path.push_str(&suffix);
    let executable = QualifiedExecutableProof {
        tool_id: tool.id.clone(),
        tool_version: tool.version.clone(),
        platform: CheckPlatform::LinuxX64,
        declared: declaration.clone(),
        observed: QualifiedExecutableObservation {
            name: declaration.name.clone(),
            path: PathBuf::from(observed_path),
            sha256: declaration.sha256.clone(),
        },
        stdout_digest: velnor_actions_contract::digest_b3(format!("{expected}\n").as_bytes()),
        stdout: format!("{expected}\n"),
        stderr_digest: velnor_actions_contract::digest_b3(b""),
    };
    receipt(
        tool,
        CheckPlatform::LinuxX64,
        platform.artifacts.clone(),
        platform.dependency_artifacts.clone(),
        platform.install_tree_sha256.clone(),
        vec![executable],
    )
    .expect("qualified proof matches declaration")
}

fn admitted_declarations(profile: Option<&HostContainerProfile>) -> Vec<QualifiedTool> {
    let mut admitted = Vec::new();
    for count in 1..=64 {
        let declarations: Vec<_> = (0..count)
            .map(|index| declaration(format!("tool-{index:03}")))
            .collect();
        let check = profile.map_or_else(
            || selected_check(&declarations),
            |profile| selected_container_check(&declarations, profile.clone()),
        );
        let bound =
            velnor_actions_contract::check_execution_receipt_upper_bound(&check, &declarations)
                .expect("receipt estimate");
        if bound > MAX_CHECK_EXECUTION_RECEIPT_BYTES {
            break;
        }
        admitted = declarations;
    }
    admitted
}

#[test]
fn maximum_admitted_receipt_survives_the_staged_gate_reader() {
    let declarations = admitted_declarations(None);
    assert!(declarations.len() > 1, "bounded closures remain usable");
    let check = selected_check(&declarations);
    let bound = velnor_actions_contract::check_execution_receipt_upper_bound(&check, &declarations)
        .expect("receipt estimate");
    assert!(bound <= MAX_CHECK_EXECUTION_RECEIPT_BYTES);
    let qualified_tools = declarations.iter().map(proof).collect::<Vec<_>>();
    let (temp, plan) = staged_with_tools(true, &declarations, &qualified_tools);
    let bytes = fs::read(artifact(&temp, &plan, "check-execution.json")).expect("receipt");
    assert!(
        bytes.len() > 128 * 1024,
        "test exercises a substantial receipt"
    );
    assert!(bytes.len() <= MAX_CHECK_EXECUTION_RECEIPT_BYTES);
    assert_eq!(verdict(&assembled(&temp)).status, FinalStatus::Passed);
}

#[test]
fn maximum_container_admitted_receipt_survives_the_staged_gate_reader() {
    let profile = crate::check_evidence::gate::container::budget_test_profile();
    let declarations = admitted_declarations(Some(&profile));
    assert!(!declarations.is_empty(), "container-qualified probes fit");
    let check = selected_container_check(&declarations, profile.clone());
    let bound = velnor_actions_contract::check_execution_receipt_upper_bound(&check, &declarations)
        .expect("container receipt estimate");
    assert!(bound <= MAX_CHECK_EXECUTION_RECEIPT_BYTES);
    let qualified_tools = declarations.iter().map(proof).collect::<Vec<_>>();
    let container = crate::check_evidence::gate::container::budget_test_receipt();
    let (temp, plan) =
        staged_with_container(true, &declarations, &qualified_tools, &profile, &container);
    let bytes = fs::read(artifact(&temp, &plan, "check-execution.json")).expect("receipt");
    assert!(
        bytes.len() > 128 * 1024,
        "test exercises container and tool proofs"
    );
    assert!(bytes.len() <= MAX_CHECK_EXECUTION_RECEIPT_BYTES);
    assert_eq!(verdict(&assembled(&temp)).status, FinalStatus::Passed);
}
