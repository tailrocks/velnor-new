//! Named-check workflow topology and failure inventory regressions.

use super::*;
use velnor_actions_contract::config::{
    CheckPlatform, CheckRunner, MiseCheck, NativeHomebrewCheck, NativeHomebrewOperation,
    QualifiedTool,
};
use velnor_actions_contract::{StepKind, VelnorConfig};

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

fn fixture_with_tools(
    checks: &[MiseCheck],
    tools: &[QualifiedTool],
) -> (tempfile::TempDir, Discovery) {
    let root = tempfile::tempdir().expect("temporary repository");
    let source = "[tasks.\"check:all\"]\nrun = 'true'\n";
    std::fs::write(root.path().join("mise.toml"), source).expect("native task source");
    let checks = velnor_actions_mise::discover_checks(root.path(), checks, tools)
        .expect("discover explicit checks");
    let discovery = Discovery {
        proposals: checks.iter().map(|row| row.proposal.clone()).collect(),
        mise_checks: checks,
        native_checks: Vec::new(),
        ..Discovery::default()
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

#[test]
fn no_cargo_checks_gate_required_without_becoming_rust_jobs() {
    let (root, discovery) = fixture(&[check("ffi", hosted("macos-15", CheckPlatform::MacosArm64))]);
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'velnor-repository-v1'\n",
    )
    .expect("config");
    let config = crate::config::load_config(root.path()).expect("defaulted config");
    let workflow = named_workflow(root.path(), &config, &discovery);
    assert_eq!(workflow.ir.jobs["check-ffi"].display_name, "Check / ffi");
    assert!(
        workflow.ir.jobs["required"]
            .needs
            .iter()
            .any(|id| id == "check-ffi")
    );
    assert!(!workflow.ir.jobs.keys().any(|id| id.starts_with("rust-")));
    let proposal = &discovery.proposals[0];
    let owners = crate::crate_job_ids::JobOwners::new(&discovery.proposals).expect("owners");
    let owner = owners.for_member(proposal).expect("check owner");
    assert_eq!(owner, Some("check-ffi".to_owned()));
    assert_eq!(workflow.ir.jobs["plan"].runs_on, "ubuntu-26.04");
    assert!(
        !workflow.ir.jobs["plan"]
            .steps
            .iter()
            .any(|step| step.name == "Prepare Rust components")
    );
    assert_eq!(workflow.ir.jobs["required"].runs_on, "ubuntu-26.04");
}

#[test]
fn mixed_platform_checks_keep_exact_tools_and_unconditional_reports() {
    let (_root, discovery) = fixture(&[
        check("linux", hosted("ubuntu-26.04", CheckPlatform::LinuxX64)),
        check("mac", hosted("macos-15", CheckPlatform::MacosArm64)),
    ]);
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("mixed platform checks");
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].1.runs_on, "ubuntu-26.04");
    assert_eq!(jobs[1].1.runs_on, "macos-15");
    for (id, job) in &jobs {
        assert_eq!(job.needs, ["plan"]);
        let checkout = job.steps.first().expect("checkout");
        let StepKind::Action { with, .. } = &checkout.kind else {
            panic!("checkout action");
        };
        assert_eq!(
            with.get("fetch-depth").map(String::as_str),
            Some("0"),
            "named check verifies the plan head against the merge commit parent"
        );
        let execution = job
            .steps
            .iter()
            .find(|step| step.name == "Execute named check")
            .expect("execution");
        assert_eq!(
            execution.condition, None,
            "Rust coverage cannot suppress opaque checks"
        );
        let StepKind::Shell { run, env } = &execution.kind else {
            panic!("qualified helper");
        };
        assert!(run.iter().any(|arg| arg.contains("velnor-actions-")));
        assert!(
            run.iter().all(|arg| !arg.contains("date +%s%3N")),
            "native check execution does not use the Ubuntu task timestamp wrapper"
        );
        assert_eq!(
            env.get(INTERNAL_OP_ENV).map(String::as_str),
            Some("execute-check-v1")
        );
        let last = job.steps.last().expect("report upload");
        assert_eq!(last.condition.as_deref(), Some("always()"));
        let StepKind::Action { with, .. } = &last.kind else {
            panic!("pinned report upload");
        };
        assert!(with.values().any(|value| value.contains(id)));
        assert!(
            !job.steps
                .iter()
                .any(|step| step.name == "Prepare pinned tools")
        );
        let row = discovery
            .mise_checks
            .iter()
            .find(|row| format!("check-{}", row.check.id) == *id)
            .expect("bound check declaration");
        assert_eq!(
            row.tool_specs,
            [format!("aqua:cli/cli@{}", velnor_actions_mise::GH_VERSION)]
        );
        assert_eq!(row.selected_rust_version(), None);
        assert_eq!(env.get("VELNOR_TASK_ID"), Some(&row.proposal.task_id));
        assert_eq!(job.check_runner.as_ref(), Some(&row.check.runner));
        assert!(
            row.qualified_tools[0]
                .platforms
                .iter()
                .any(|p| p.platform == row.check.runner.platform)
        );
        assert!(
            job.steps.contains(
                &crate::matrix_step::download_plan_step().expect("Plan artifact binding")
            )
        );
    }
}

#[test]
fn native_check_job_uses_closed_sdk_step_and_standard_plan_transport() {
    let check = NativeHomebrewCheck {
        id: "tap-tree".to_owned(),
        operation: NativeHomebrewOperation::TapTree,
        formula_channel: None,
        runner: hosted("macos-15", CheckPlatform::MacosArm64),
        timeout_minutes: 1,
    };
    let discovery = Discovery {
        native_checks: vec![check],
        ..Discovery::default()
    };
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("native check job");
    assert_eq!(jobs.len(), 1);
    let (id, job) = &jobs[0];
    assert_eq!(id, "check-tap-tree");
    assert_eq!(job.needs, ["plan"]);
    let step = job
        .steps
        .iter()
        .find(|step| matches!(step.kind, StepKind::NativeCheckExecution { .. }))
        .expect("typed native execution");
    let StepKind::NativeCheckExecution {
        check_id,
        operation,
        task_id,
        ..
    } = &step.kind
    else {
        panic!("native check execution variant");
    };
    assert_eq!(check_id, "tap-tree");
    assert_eq!(*operation, NativeHomebrewOperation::TapTree);
    assert_eq!(task_id, "internal/native-check/tap-tree");
    assert_eq!(
        job.steps.last().and_then(|step| step.condition.as_deref()),
        Some("always()")
    );
}

#[test]
fn generic_registry_id_generates_without_catalog_fallback_and_keeps_plan_identity() {
    let mut declared = gh_qualification();
    let old_version = declared.version.clone();
    declared.id = "consumer-gh".into();
    declared.version = "2.101.0".into();
    for platform in &mut declared.platforms {
        for artifact in &mut platform.artifacts {
            artifact.url = artifact.url.replace(&old_version, &declared.version);
        }
        platform.executables[0].probe =
            velnor_actions_contract::config::QualifiedToolProbe::Version {
                expected: "gh version 2.101.0".into(),
            };
    }
    let mut check = check("generic", hosted("ubuntu-26.04", CheckPlatform::LinuxX64));
    check.tools = vec![declared.id.clone()];
    let (_root, discovery) = fixture_with_tools(&[check], &[declared.clone()]);
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("opaque declared tool ID must generate");
    let row = &discovery.mise_checks[0];
    assert_eq!(row.qualified_tools, [declared]);
    assert_eq!(row.tool_specs, ["aqua:cli/cli@2.101.0"]);
    let job = &jobs[0].1;
    assert!(
        job.steps
            .contains(&crate::matrix_step::download_plan_step().expect("Plan artifact"))
    );
    assert!(!job.steps.iter().any(|s| s.name == "Prepare pinned tools"));
    let execute = job
        .steps
        .iter()
        .find(|s| s.name == "Execute named check")
        .expect("qualified task");
    let StepKind::Shell { env, .. } = &execute.kind else {
        panic!("qualified helper");
    };
    assert_eq!(env.get("VELNOR_TASK_ID"), Some(&row.proposal.task_id));
    assert_eq!(
        env.get(INTERNAL_OP_ENV).map(String::as_str),
        Some("execute-check-v1")
    );
    assert_eq!(execute.condition, None);
    assert_eq!(
        job.steps.last().and_then(|s| s.condition.as_deref()),
        Some("always()")
    );
}

#[path = "check_jobs_admission_tests.rs"]
mod admission_tests;
#[path = "check_jobs_shellcheck_tests.rs"]
mod shellcheck_tests;
