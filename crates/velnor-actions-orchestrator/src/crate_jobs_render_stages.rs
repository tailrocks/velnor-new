//! Ordered preparation, source restore, and obligation rendering stages.

use crate::OrchestratorError;
use velnor_actions_contract::{CrateJob, Step, WorkflowPolicy};
use velnor_actions_mise::{TaskCacheMode, ToolCatalog};

/// Validated selections and scoped inputs shared by setup stages.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent driver selections preserve rendering inputs"
)]
pub(super) struct Setup<'a> {
    pub(super) label: &'a str,
    pub(super) policy: WorkflowPolicy,
    pub(super) model: &'a CrateJob,
    pub(super) catalog: &'a ToolCatalog,
    pub(super) fetch_roots: &'a [String],
    pub(super) use_rust: bool,
    pub(super) use_mbx: bool,
    pub(super) use_nextest: bool,
    pub(super) use_opentofu: bool,
    pub(super) provider_descriptor: Option<&'a crate::tofu_cache_source::ProviderExportDescriptor>,
}

/// Stage helper, plan, native restores, and selected tools in order.
pub(super) fn append_preparation(
    steps: &mut Vec<Step>,
    setup: &Setup<'_>,
    acquire: Option<&Step>,
) -> Result<(), OrchestratorError> {
    steps.push(crate::workflow::wire_w1::checkout_step()?);
    steps.extend(acquire.cloned());
    steps.push(crate::matrix_step::download_plan_step()?);
    let rust_package = setup
        .model
        .obligations
        .iter()
        .any(|obligation| obligation.task_id.starts_with("stack/rust/"))
        .then_some(setup.model.package_name.as_str());
    let suite = crate::matrix_step::crate_suite_tools(setup.policy, rust_package)?;
    steps.push(crate::matrix_step::prepare_crate_tools_step(
        setup.catalog,
        setup.use_rust,
        setup.use_mbx,
        setup.use_nextest,
        setup.use_opentofu,
        suite,
        setup.label,
    )?);
    steps.extend(crate::workloads::prepare_step(
        setup.model,
        setup.catalog,
        setup.label,
    )?);
    if setup.model.configuration == "node_ci" {
        steps.push(crate::workloads::node_materialization_step(
            &setup.model.manifest,
        )?);
    }
    if setup.use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(
            setup.catalog,
        )?);
    }
    Ok(())
}

/// Restore providers, then fetch Rust sources; MBX admission follows tool normalization.
pub(super) fn append_sources(
    steps: &mut Vec<Step>,
    setup: &Setup<'_>,
) -> Result<(), OrchestratorError> {
    if setup.use_opentofu {
        let root = crate::tofu_cache::tofu_root_for_obligations(&setup.model.obligations)?;
        steps.extend(crate::tofu_cache::prepare_root_steps(
            setup.label,
            setup.catalog,
            &root,
            setup.provider_descriptor,
        )?);
    }
    if setup.use_rust {
        steps.extend(crate::source_prep::fetch_steps_for_crate(
            setup.catalog,
            setup.fetch_roots,
        )?);
    }
    steps.extend(crate::workflow::wire_w1::maybe_task_cache_steps(
        None,
        TaskCacheMode::Off,
        "",
    )?);
    Ok(())
}

/// Append report-wrapped obligations with unchanged downstream gate scope.
pub(super) fn append_obligations(
    steps: &mut Vec<Step>,
    model: &CrateJob,
    catalog: &ToolCatalog,
    use_opentofu: bool,
    max_parallel_jobs: u32,
    bindings: &crate::crate_jobs::helpers::SourceBindings,
) -> Result<Vec<velnor_actions_contract::CompiledSourceHelper>, OrchestratorError> {
    bindings.validate_coverage(&model.obligations)?;
    let mut records = Vec::new();
    for (index, obligation) in model.obligations.iter().enumerate() {
        let downstream: Vec<String> = model.obligations[index + 1..]
            .iter()
            .map(|later| later.task_id.clone())
            .collect();
        // The first obligation declares the root job's concurrency cap;
        // the renderer turns the marker into `strategy.max-parallel`.
        let cap = (index == 0 && use_opentofu).then_some(max_parallel_jobs);
        if let Some(recipe) = bindings.execution_for(obligation)? {
            match recipe {
                crate::crate_jobs::helpers::ExecutionRecipe::NativeHelper { record, .. } => {
                    steps.extend(crate::helper_obligation_steps::steps(
                        obligation,
                        catalog,
                        &downstream,
                        cap,
                        record,
                    )?);
                    records.push(record.as_ref().clone());
                }
                crate::crate_jobs::helpers::ExecutionRecipe::AlreadyReportedCompiler(recipe) => {
                    let record = recipe.bind_frame(obligation, catalog, &downstream, cap)?;
                    let mut step =
                        velnor_actions_workflow_renderer::source_helper::source_helper_step(
                            &obligation.step_name,
                            &record,
                            record.environment().clone(),
                        )?;
                    let uncovered = crate::covered_tasks::skip_condition(&obligation.task_id)?;
                    step.condition = Some(format!("success() && ({uncovered})"));
                    steps.push(step);
                    records.push(record);
                }
            }
        } else if let Some(action) =
            crate::obligation_steps::steps(model, obligation, catalog, &downstream, cap)?
        {
            steps.extend(action);
        } else {
            steps.push(crate::matrix_step::obligation_step(
                obligation,
                catalog,
                &downstream,
                cap,
            )?);
        }
    }
    Ok(records)
}
