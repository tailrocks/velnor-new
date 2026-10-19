use super::{
    CrateJob, Job, JobTimeout, OrchestratorError, Step, TaskCacheMode, ToolCatalog, WorkflowPolicy,
};

#[expect(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "one call site threads job scope plus driver selection"
)]
pub(super) fn render_job(
    label: &str,
    policy: WorkflowPolicy,
    model: &CrateJob,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    acquire: Option<&Step>,
    max_parallel_jobs: u32,
    helper_version: &str,
    cache_policy: velnor_actions_contract::PullRequestCachePolicy,
    timeout: JobTimeout,
    needs: Vec<String>,
    qualification: bool,
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.extend(acquire.cloned());
    steps.push(crate::matrix_step::download_plan_step()?);
    let suite: Option<crate::matrix_step::CrateSuite> =
        crate::matrix_step::suite_for_package(&model.package_name);
    let needs_validators = crate::matrix_step::crate_needs_generate_validators(policy, suite);
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        catalog,
        use_rust,
        use_nextest,
        crate::matrix_step::prepare_install_opentofu(policy, suite, use_opentofu),
        needs_validators,
    )?);
    if use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    }
    steps.extend(crate::source_cache::restore_step_for_crate(
        label,
        catalog,
        fetch_roots,
        use_rust,
    )?);
    if use_opentofu {
        let root = crate::tofu_cache::tofu_root_for_obligations(&model.obligations)?;
        steps.extend(crate::tofu_cache::provider_cache_step_for_tofu_root(
            label, catalog, &root,
        )?);
    }
    if use_mbx {
        let root = crate::source_prep::workspace_root_for_manifest(&model.manifest, fetch_roots);
        let working_dir = (!root.is_empty()).then_some(root);
        steps.extend(crate::mbx_preflight::steps_for_catalog(
            catalog,
            cache_policy,
            working_dir,
        )?);
    }
    if use_rust {
        let all_features = obligations_use_all_features(&model.obligations);
        let scoped_roots = crate::source_prep::crate_fetch_roots(
            &model.manifest,
            fetch_roots,
            policy == WorkflowPolicy::VelnorRepositoryV1 && needs_validators,
        );
        steps.extend(crate::source_prep::fetch_steps_for_crate_with_all_features(
            catalog,
            &scoped_roots,
            all_features,
        )?);
    }
    steps.extend(render_terminal_steps(
        model,
        catalog,
        use_opentofu,
        max_parallel_jobs,
        helper_version,
        qualification,
    )?);
    steps.push(crate::matrix_step::crate_upload_step(&model.job_id)?);
    Ok(Job {
        display_name: model.display_name.clone(),
        runs_on: label.to_owned(),
        check_runner: None,
        timeout_minutes: timeout,
        needs,
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    })
}

fn render_terminal_steps(
    model: &CrateJob,
    catalog: &ToolCatalog,
    use_opentofu: bool,
    max_parallel_jobs: u32,
    helper_version: &str,
    qualification: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = crate::workflow::wire_w1::maybe_task_cache_steps(None, TaskCacheMode::Off, "")?;
    steps.extend(render_obligation_steps(
        model,
        catalog,
        use_opentofu,
        max_parallel_jobs,
        helper_version,
    )?);
    if qualification {
        let mut source_evidence =
            crate::qualification_source_evidence::source_evidence_step(catalog)?;
        source_evidence.condition =
            Some("needs.plan.outputs.qualification_campaign != ''".to_owned());
        steps.push(source_evidence);
    }
    Ok(steps)
}

fn obligations_use_all_features(
    obligations: &[velnor_actions_contract::workflow::CrateObligation],
) -> bool {
    obligations
        .iter()
        .any(|obligation| obligation.run.iter().any(|arg| arg == "--all-features"))
}

fn render_obligation_steps(
    model: &CrateJob,
    catalog: &ToolCatalog,
    use_opentofu: bool,
    max_parallel_jobs: u32,
    helper_version: &str,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = Vec::with_capacity(model.obligations.len());
    for (index, obligation) in model.obligations.iter().enumerate() {
        // The first obligation declares the root job's concurrency cap;
        // the renderer turns the marker into `strategy.max-parallel`.
        let cap = (index == 0 && use_opentofu).then_some(max_parallel_jobs);
        let parallel =
            !use_opentofu && !velnor_actions_rust::runs_before_parallel_checks(&obligation.kind);
        let target_lane = parallel
            .then(|| velnor_actions_rust::concurrent_target_lane(&obligation.kind))
            .flatten();
        let mut step = crate::matrix_step::obligation_step(
            obligation,
            catalog,
            cap,
            helper_version,
            target_lane,
        )?;
        step.parallel = parallel;
        steps.push(step);
    }
    Ok(steps)
}
