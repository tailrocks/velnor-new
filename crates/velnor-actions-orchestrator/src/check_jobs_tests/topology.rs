use super::*;

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
    assert_eq!(
        crate::crate_job_ids::job_id_for_member(
            &discovery.proposals,
            &discovery.proposals[0],
            velnor_actions_contract::WorkflowEvent::Push,
        ),
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
fn mixed_rust_tofu_and_named_checks_bind_qualification_plan_to_required_jobs() {
    let mut java = check("java", hosted("ubuntu-26.04", CheckPlatform::LinuxX64));
    java.task = "check:java".to_owned();
    let mut frontend = check("frontend", hosted("ubuntu-26.04", CheckPlatform::LinuxX64));
    frontend.task = "check:frontend".to_owned();
    let (root, mut discovery) = fixture(&[java, frontend]);
    discovery
        .proposals
        .extend([rust_proposal(), tofu_proposal()]);
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'velnor-repository-v1'\n",
    )
    .expect("config");
    let config = crate::config::load_config(root.path()).expect("defaulted config");
    let workflow = named_workflow(root.path(), &config, &discovery);

    let required: std::collections::BTreeSet<&str> = workflow.ir.jobs["required"]
        .needs
        .iter()
        .map(String::as_str)
        .collect();
    let mut planned = Vec::new();
    for task in &discovery.proposals {
        let job_id = crate::crate_job_ids::job_id_for_member(
            &discovery.proposals,
            task,
            WorkflowEvent::Qualification,
        )
        .expect("Qualification plan owner");
        assert!(workflow.ir.jobs.contains_key(&job_id), "{job_id}");
        assert!(
            required.contains(job_id.as_str()),
            "Required omits {job_id}"
        );
        planned.push(job_id);
    }
    planned.sort();
    let required_qualification: Vec<&str> = required
        .iter()
        .copied()
        .filter(|job_id| {
            job_id.starts_with("rust-qualification-")
                || job_id.starts_with("tofu-qualification-")
                || job_id.starts_with(velnor_actions_contract::QUALIFICATION_CHECK_JOB_PREFIX)
        })
        .collect();
    assert_eq!(
        required_qualification,
        planned.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(
        workflow.ir.jobs["qualification-check-java"].check_runner,
        workflow.ir.jobs["check-java"].check_runner
    );
    assert_eq!(
        workflow.ir.jobs["qualification-check-frontend"].check_runner,
        workflow.ir.jobs["check-frontend"].check_runner
    );

    assert_named_check_plan_entries(root.path(), &discovery, &required);
}
