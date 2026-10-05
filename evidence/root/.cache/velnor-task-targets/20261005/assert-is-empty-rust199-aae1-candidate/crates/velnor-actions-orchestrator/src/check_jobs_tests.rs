//! Named-check workflow topology and failure inventory regressions.

use super::*;
use velnor_actions_contract::config::{CheckPlatform, CheckRunner, MiseCheck, QualifiedTool};
use velnor_actions_contract::{JobConclusion, RequiredJobResult, StepKind};
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
        clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
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
    let workflow =
        crate::workflow::build_workflow(&config, "main", "ubuntu-26.04", &discovery, &[])
            .expect("named checks workflow");
    assert_eq!(workflow.ir.jobs["check-ffi"].display_name, "Check / ffi");
    assert!(
        workflow.ir.jobs["required"]
            .needs
            .iter()
            .any(|id| id == "check-ffi")
    );
    assert!(!workflow.ir.jobs.keys().any(|id| id.starts_with("rust-")));
    assert_eq!(
        crate::crate_job_ids::job_id_for_member(&discovery.proposals, &discovery.proposals[0]),
        Some("check-ffi".to_owned())
    );
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

#[test]
fn fork_admission_skips_external_runner_and_required_rejects_skip() {
    let runner = CheckRunner {
        label: "native-check-scale-set".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container: None,
    };
    let (_root, discovery) = fixture(&[check("native", runner)]);
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("external check");
    assert_eq!(
        jobs[0].1.condition.as_deref(),
        Some(EPHEMERAL_CHECK_ADMISSION_CONDITION)
    );
    assert!(
        EPHEMERAL_CHECK_ADMISSION_CONDITION.contains("head.repo.full_name == github.repository")
    );
    let mut signals = crate::cover::Signals::default();
    crate::merge::required_evidence::fold_jobs(
        &[RequiredJobResult {
            job_id: "check-native".to_owned(),
            conclusion: JobConclusion::Skipped,
        }],
        &mut signals,
    );
    assert!(signals.not_run);
}

#[test]
fn required_rejects_failed_or_missing_named_check() {
    for conclusion in [JobConclusion::Missing, JobConclusion::Failure] {
        let mut signals = crate::cover::Signals::default();
        crate::merge::required_evidence::fold_jobs(
            &[RequiredJobResult {
                job_id: "check-ffi".to_owned(),
                conclusion,
            }],
            &mut signals,
        );
        assert!(signals.failed, "{conclusion:?}");
    }
}

#[test]
fn native_check_without_catalog_tools_adds_no_rust_install() {
    let mut native = check("native", hosted("macos-15", CheckPlatform::MacosArm64));
    native.tools.clear();
    let (_root, discovery) = fixture(&[native]);
    let jobs = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("native system check");
    let names: Vec<&str> = jobs[0]
        .1
        .steps
        .iter()
        .map(|step| step.name.as_str())
        .collect();
    assert!(!names.contains(&"Prepare pinned tools"));
    assert!(!names.contains(&"Prepare Rust components"));
    assert!(names.contains(&"Execute named check"));
}

#[test]
fn ignored_rust_candidate_keeps_plan_inventory_toolchain() {
    use velnor_actions_contract::{DetectedProject, DetectionStatus};
    let (root, mut discovery) = fixture(&[]);
    discovery.statuses.push(DetectionStatus::Ignored {
        project: DetectedProject {
            stack_id: "rust".to_owned(),
            project_root: String::new(),
            manifest: "Cargo.toml".to_owned(),
        },
        reason: "configured_ignore".to_owned(),
    });
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'velnor-repository-v1'\n",
    )
    .expect("config");
    let config = crate::config::load_config(root.path()).expect("defaulted config");
    let workflow =
        crate::workflow::build_workflow(&config, "main", "ubuntu-26.04", &discovery, &[])
            .expect("ignored Rust workflow");
    let plan = &workflow.ir.jobs["plan"];
    assert!(
        plan.steps
            .iter()
            .any(|step| step.name == "Prepare Rust components")
    );
    assert!(plan.steps.iter().any(|step| match &step.kind {
        StepKind::Shell { run, .. } =>
            run.contains(&ToolCatalog::pinned().tool_spec(PinnedTool::Rust)),
        _ => false,
    }));
}
