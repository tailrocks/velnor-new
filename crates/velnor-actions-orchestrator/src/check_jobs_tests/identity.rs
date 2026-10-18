use super::*;

#[test]
fn qualification_named_check_keeps_command_but_uses_its_own_report_identity() {
    let (_root, discovery) = fixture(&[check(
        "java",
        hosted("ubuntu-26.04", CheckPlatform::LinuxX64),
    )]);
    let normal = build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("normal check");
    let qualification = build_qualification_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("Qualification check");
    let (normal_id, normal_job) = &normal[0];
    let (qualification_id, qualification_job) = &qualification[0];
    assert_eq!(normal_id, "check-java");
    assert_eq!(qualification_id, "qualification-check-java");
    assert_eq!(normal_job.runs_on, qualification_job.runs_on);
    assert_eq!(normal_job.check_runner, qualification_job.check_runner);

    let (
        StepKind::Shell {
            run: normal_run,
            env: normal_env,
        },
        StepKind::Shell {
            run: qualification_run,
            env: qualification_env,
        },
    ) = (
        &named_step(normal_job).kind,
        &named_step(qualification_job).kind,
    )
    else {
        panic!("named check command");
    };
    assert_eq!(normal_run, qualification_run);
    let mut qualification_env = qualification_env.clone();
    qualification_env.remove(NAMED_CHECK_JOB_ID_ENV);
    let mut normal_env = normal_env.clone();
    normal_env.remove(NAMED_CHECK_JOB_ID_ENV);
    assert_eq!(normal_env, qualification_env);
    assert_eq!(normal_env["VELNOR_CHECK_ID"], "java");

    let upload_name = |job: &velnor_actions_contract::Job| {
        let StepKind::Action { with, .. } = &job.steps.last().expect("upload step").kind else {
            panic!("report upload");
        };
        with["name"].clone()
    };
    assert_ne!(upload_name(normal_job), upload_name(qualification_job));
    assert!(upload_name(qualification_job).ends_with("-qualification-check-java"));
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
    let workflow = named_workflow(root.path(), &config, &discovery);
    let plan = &workflow.ir.jobs["plan"];
    let pinned_tools = plan
        .steps
        .iter()
        .find(|step| step.name == "Prepare pinned tools")
        .expect("plan inventory prepares its exact Rust compiler");
    assert!(matches!(
        &pinned_tools.kind,
        StepKind::Shell { run, .. }
            if run.contains(&ToolCatalog::pinned().tool_spec(PinnedTool::Rust))
    ));
    assert!(
        !plan
            .steps
            .iter()
            .any(|step| step.name == "Prepare Rust components"),
        "an ignored candidate has no selected plan-owned Format step"
    );
}
