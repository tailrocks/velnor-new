//! Named-check workflow topology and failure inventory regressions.

use super::*;
use velnor_actions_contract_config::config::{
    CheckPlatform, CheckRunner, MiseCheck, QualifiedTool,
};
use velnor_actions_contract_workflow::{ARTIFACT_MATRIX_PROVIDER_ENV, StepKind};
use velnor_actions_mise::PinnedTool;

fn check(id: &str, runner: CheckRunner) -> MiseCheck {
    MiseCheck {
        id: id.to_owned(),
        task: "check:all".to_owned(),
        directory: ".".to_owned(),
        runner,
        inputs: vec!["mise.toml".to_owned()],
        tools: vec!["gh".to_owned()],
        system_tools: Vec::new(),
        evidence: None,
        timeout_minutes: 20,
    }
}

fn hosted(label: &str, platform: CheckPlatform) -> CheckRunner {
    CheckRunner {
        label: label.to_owned(),
        platform,
        executor: CheckExecutor::Hosted,
        container: None,
    }
}

fn fixture(checks: &[MiseCheck]) -> (tempfile::TempDir, Discovery) {
    fixture_with_tools(checks, &[gh_qualification()])
}

fn fixture_with_tools(
    checks: &[MiseCheck],
    tools: &[QualifiedTool],
) -> (tempfile::TempDir, Discovery) {
    let root = tempfile::tempdir().expect("temporary repository");
    std::fs::write(
        root.path().join("mise.toml"),
        "[tasks.\"check:all\"]\nrun = 'true'\n",
    )
    .expect("native task source");
    let checks = velnor_actions_mise::discover_checks(root.path(), checks, tools)
        .expect("discover explicit checks");
    let discovery = Discovery {
        proposals: checks.iter().map(|row| row.proposal.clone()).collect(),
        mise_checks: checks,
        statuses: Vec::new(),
        workspaces: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: velnor_actions_orchestrator_core::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    };
    (root, discovery)
}

fn gh_qualification() -> velnor_actions_contract_config::config::QualifiedTool {
    use velnor_actions_contract_config::config::{
        QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend, QualifiedToolExecutable,
        QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe,
    };
    let version = velnor_actions_mise::GH_VERSION;
    QualifiedTool {
        id: "gh".into(),
        backend: QualifiedToolBackend::Aqua {
            package: "cli/cli".into(),
        },
        version: version.into(),
        options: QualifiedToolOptions::Default,
        depends_on: vec![],
        platforms: [
            CheckPlatform::LinuxX64,
            CheckPlatform::MacosArm64,
            CheckPlatform::MacosX64,
        ]
        .into_iter()
        .map(|platform| QualifiedToolPlatform {
            platform,
            artifacts: vec![QualifiedToolArtifact {
                url: format!(
                    "https://github.com/cli/cli/releases/download/v{version}/gh_{version}_{}",
                    match platform {
                        CheckPlatform::LinuxX64 => "linux_amd64.tar.gz",
                        CheckPlatform::MacosArm64 => "macOS_arm64.zip",
                        CheckPlatform::MacosX64 => "macOS_amd64.zip",
                    }
                ),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: vec![],
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: "gh".into(),
                path: "bin/gh".into(),
                sha256: "c".repeat(64),
                probe: QualifiedToolProbe::Version {
                    expected: format!("gh version {version}"),
                },
            }],
        })
        .collect(),
    }
}

mod check_jobs_tests;
