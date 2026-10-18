//! Named-check workflow topology and failure inventory regressions.

use super::*;
use velnor_actions_contract::config::{CheckPlatform, CheckRunner, MiseCheck, QualifiedTool};
use velnor_actions_contract::{
    JobConclusion, RequiredJobResult, StepKind, VelnorConfig, WorkflowEvent,
};
use velnor_actions_mise::PinnedTool;
use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};

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
        timeout_minutes: 5,
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

fn named_workflow(
    root: &std::path::Path,
    config: &VelnorConfig,
    discovery: &Discovery,
) -> crate::workflow::WorkflowPlan {
    crate::workflow::build_workflow(root, config, "main", "ubuntu-26.04", discovery, &[])
        .expect("named checks workflow")
}

fn named_step(job: &velnor_actions_contract::Job) -> &velnor_actions_contract::Step {
    job.steps
        .iter()
        .find(|step| step.name == "Execute named check")
        .expect("named check step")
}

fn assert_named_check_plan_entries(
    root: &std::path::Path,
    discovery: &Discovery,
    required: &std::collections::BTreeSet<&str>,
) {
    for item in &discovery.mise_checks {
        let job_id = crate::crate_job_ids::job_id_for_member(
            &discovery.proposals,
            &item.proposal,
            WorkflowEvent::Qualification,
        )
        .expect("named-check plan owner");
        let lane = velnor_actions_contract::NamedCheckLane {
            variant: None,
            job_id: job_id.clone(),
            runs_on: Some(item.check.runner.label.clone()),
        };
        let (_, entries) = crate::internal_plan::named_checks::plan::derive_lanes(
            root,
            item,
            "local",
            &crate::internal_plan::default_generator(),
            &ToolCatalog::pinned(),
            &[lane],
        )
        .expect("named-check plan entry");
        assert_eq!(entries.len(), 1, "{job_id}");
        assert_eq!(entries[0].job_id, job_id);
        assert!(required.contains(entries[0].job_id.as_str()));
    }
}

fn rust_proposal() -> velnor_actions_contract::ProposedTask {
    let group = TaskGroup {
        all_features: false,
        task_id: "stack/rust/root/clippy/default".to_owned(),
        package_id: "demo 0.1.0".to_owned(),
        package_name: "demo".to_owned(),
        manifest_key: "root".to_owned(),
        kind: TaskKind::Clippy,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
    };
    velnor_actions_rust::propose_task(&group).expect("Rust proposal")
}

fn tofu_proposal() -> velnor_actions_contract::ProposedTask {
    let group = velnor_actions_tofu::TofuTaskGroup {
        root: "stacks/demo".to_owned(),
        kind: velnor_actions_tofu::TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    velnor_actions_tofu::propose_task(&group).expect("Tofu proposal")
}

fn fixture_with_tools(
    checks: &[MiseCheck],
    tools: &[QualifiedTool],
) -> (tempfile::TempDir, Discovery) {
    let root = tempfile::tempdir().expect("temporary repository");
    let source = "[tasks.\"check:all\"]\nrun = 'true'\n[tasks.\"check:java\"]\nrun = 'true'\n[tasks.\"check:frontend\"]\nrun = 'true'\n";
    std::fs::write(root.path().join("mise.toml"), source).expect("native task source");
    let checks = velnor_actions_mise::discover_checks(root.path(), checks, tools)
        .expect("discover explicit checks");
    let discovery = Discovery {
        proposals: checks.iter().map(|row| row.proposal.clone()).collect(),
        mise_checks: checks,
        statuses: Vec::new(),
        workspaces: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    };
    (root, discovery)
}

fn gh_qualification() -> velnor_actions_contract::config::QualifiedTool {
    use velnor_actions_contract::config::{
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

#[path = "check_jobs_tests/identity.rs"]
mod identity;
#[path = "check_jobs_tests/topology.rs"]
mod topology;
