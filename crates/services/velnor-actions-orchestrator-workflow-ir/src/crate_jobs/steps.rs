//! Setup-step construction for one validated crate job.

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::{CrateJob, Step};
use velnor_actions_mise::{TaskCacheMode, ToolCatalog};
use velnor_actions_orchestrator_core::OrchestratorError;

#[expect(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    reason = "one call site threads job scope plus driver selection"
)]
pub(super) fn render_setup_steps(
    label: &str,
    policy: WorkflowPolicy,
    model: &CrateJob,
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    repo_has_mbx: bool,
    acquire: Option<&Step>,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
    steps.extend(acquire.cloned());
    steps.push(velnor_actions_orchestrator_provisioning::matrix_step::download_plan_step()?);
    let needs_validators =
        velnor_actions_orchestrator_provisioning::matrix_step::crate_needs_generate_validators(
            policy,
            &model.package_name,
        );
    steps.push(
        velnor_actions_orchestrator_provisioning::matrix_step::prepare_crate_tools_step(
            catalog,
            use_rust,
            use_nextest,
            velnor_actions_orchestrator_provisioning::matrix_step::prepare_install_opentofu(
                policy,
                &model.package_name,
                use_opentofu,
            ),
            needs_validators,
        )?,
    );
    if use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    }
    steps.extend(super::restore_step_for_crate(
        label,
        catalog,
        fetch_roots,
        use_rust,
        use_mbx,
        repo_has_mbx,
    )?);
    if use_opentofu {
        let root = velnor_actions_orchestrator_provisioning::tofu_cache::tofu_root_for_obligations(
            &model.obligations,
        )?;
        steps.extend(velnor_actions_orchestrator_provisioning::tofu_cache::provider_cache_step_for_tofu_root(
            label, catalog, &root,
        )?);
    }
    if use_mbx {
        steps.extend(crate::mbx_preflight::steps_for_catalog(catalog)?);
    }
    if use_rust {
        steps.extend(
            velnor_actions_orchestrator_provisioning::source_prep::fetch_steps_for_crate(
                catalog,
                fetch_roots,
            )?,
        );
    }
    steps.extend(crate::workflow::wire_w1::maybe_task_cache_steps(
        None,
        TaskCacheMode::Off,
        "",
    )?);
    Ok(steps)
}
