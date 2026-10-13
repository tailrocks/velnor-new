use super::*;

pub(super) fn assert_no_format_without_rustfmt_config() -> TestResult {
    let (_repo, yaml, _alint) = preview_both(config_with_branch())?;
    let plan = window(&yaml, "  plan:", "  required:")?;
    assert!(
        !plan.contains("- name: Format"),
        "no workspace scope without rustfmt config:\n{plan}"
    );
    Ok(())
}

pub(super) fn assert_configured_format_is_per_crate() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join("rustfmt.toml"), "[rustfmt]\n")?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let workflow_env = window(yaml, "env:\n", "concurrency:")?;
    let plan = window(yaml, "  plan:", "  required:")?;
    let job = window(yaml, "  rust-demo:", "  required:")?;
    assert_no_overlapping_format(plan, yaml)?;
    assert!(job.contains("- name: Format"), "per-package scope:\n{job}");
    assert_format_task_is_manifest_bound(&prep, &tree, job)?;
    assert_format_environment(workflow_env);
    assert_format_count_matches_crate_jobs(&prep, yaml);
    Ok(())
}

fn assert_no_overlapping_format(plan: &str, yaml: &str) -> TestResult {
    assert!(
        !plan.contains("- name: Format"),
        "root package owns its fmt scope:\n{plan}"
    );
    assert!(
        !yaml.contains("fmt --all --check"),
        "no workspace-wide fmt:\n{yaml}"
    );
    Ok(())
}

fn assert_format_task_is_manifest_bound(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
    tree: &velnor_actions_workflow_renderer::tree::RenderedTree,
    job: &str,
) -> TestResult {
    let rust_job = prep
        .workflow
        .ir
        .jobs
        .get("rust-demo")
        .ok_or("missing rust-demo IR job")?;
    let format_task = rust_job
        .steps
        .iter()
        .find(|step| step.name == "Format")
        .ok_or("missing Format task")?;
    let StepKind::TaskExecution { task_id, .. } = &format_task.kind else {
        return Err("Format must be a declared task".into());
    };
    crate::impl_wire_w1_manifest::assert_format_task_record(task_id, job, tree)
}

fn assert_format_environment(workflow_env: &str) {
    for key in ["MISE_AUTO_INSTALL:", "MISE_EXEC_AUTO_INSTALL:"] {
        assert!(
            workflow_env.contains(key),
            "workflow env misses {key}:\n{workflow_env}"
        );
    }
}

fn assert_format_count_matches_crate_jobs(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
    yaml: &str,
) {
    let formats = yaml.matches("- name: Format").count();
    let crates = prep
        .workflow
        .ir
        .jobs
        .keys()
        .filter(|id| id.starts_with("rust-"))
        .count();
    assert!(crates >= 1, "fixture needs a crate job");
    assert_eq!(formats, crates, "one Format per crate, none elsewhere");
}

pub(super) fn assert_virtual_workspace_has_no_overlap() -> TestResult {
    // Per-package groups own every file; neither the plan job nor a workspace
    // formatter may overlap those scopes.
    let virtual_repo = make_virtual_repo(config_with_branch())?;
    fs::write(virtual_repo.path().join("rustfmt.toml"), "[rustfmt]\n")?;
    let prep = prepare(virtual_repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    let plan = window(yaml, "  plan:", "  required:")?;
    assert_no_overlapping_format(plan, yaml)
}

pub(super) fn assert_ignored_rust_has_no_format() -> TestResult {
    let ignored =
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n";
    let (_repo, yaml, _alint) = preview_both(ignored)?;
    assert!(
        !yaml.contains("- name: Format"),
        "no fmt without rust:\n{yaml}"
    );
    Ok(())
}
