//! Fixed-order crate job rendering.

use crate::OrchestratorError;
use velnor_actions_contract::{CrateJob, Job, JobTimeout, Step, WorkflowPolicy};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

#[path = "crate_jobs_render_stages.rs"]
mod stages;

/// Render one validated crate model to its fixed IR job.
///
/// P08 order: helper staging, plan download (report identities bind
/// the plan), restore native caches, the per-root provider restore on
/// opentofu roles, then MBX objects, then probe-and-fetch, then
/// report-wrapped obligations, then one
/// always-on crate-report upload carrying every entry. Readers never
/// save. Rust setup (components, restore, fetch) emits only for rust
/// roles; pure-tofu roles carry the opentofu driver with no Rust
/// setup, mixed roles the union.
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
    provider_descriptor: Option<&crate::tofu_cache_source::ProviderExportDescriptor>,
    bindings: &super::helpers::SourceBindings,
) -> Result<(Job, Vec<velnor_actions_contract::CompiledSourceHelper>), OrchestratorError> {
    let runner = crate::workloads::runner_for_model(model, label);
    let label = runner.as_str();
    let setup = stages::Setup {
        label,
        policy,
        model,
        catalog,
        fetch_roots,
        use_rust,
        use_mbx,
        use_nextest,
        use_opentofu,
        provider_descriptor,
    };
    let mut steps = Vec::new();
    stages::append_preparation(&mut steps, &setup, acquire)?;
    stages::append_sources(&mut steps, &setup)?;
    let records = stages::append_obligations(
        &mut steps,
        model,
        catalog,
        use_opentofu,
        max_parallel_jobs,
        bindings,
    )?;
    steps.push(crate::matrix_step::stage_reports_step());
    steps.push(crate::matrix_step::crate_upload_step(&model.job_id)?);
    Ok((
        Job {
            cache_mode: None,
            display_name: model.display_name.clone(),
            runs_on: crate::workloads::runner_for_model(model, label),
            timeout_minutes: JobTimeout::CRATE,
            needs: vec![PLAN_JOB_ID.to_owned()],
            condition: Some(crate::covered_tasks::job_condition(&model.obligations)?),
            permissions: None,
            tool_producer: None,
            mbx_producer: None,
            source_producer: None,
            native_pages_deploy: None,
            native_publish: None,
            outputs: Vec::new(),
            environment: None,
            steps,
        },
        records,
    ))
}
