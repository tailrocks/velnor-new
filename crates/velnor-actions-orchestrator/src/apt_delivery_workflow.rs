//! Closed typed APT graph: verification, isolated signing and protected Pages.
use super::AptRenderContext;
use std::collections::BTreeMap;
use velnor_actions_contract::config::AptDeliveryConfig;
use velnor_actions_contract::workflow::{Concurrency, PermissionLevel, Permissions};
use velnor_actions_contract::{CompiledSourceHelper, WorkflowIr};
use velnor_actions_workflow_renderer::{RenderError, pages_approval::NativePagesApproval};

#[path = "apt_delivery_jobs.rs"]
mod jobs;
#[path = "apt_delivery_pages.rs"]
mod pages;
#[path = "apt_delivery_records.rs"]
mod records;
#[path = "apt_delivery_steps.rs"]
mod steps;
#[path = "apt_delivery_tools.rs"]
mod tools;
#[path = "apt_delivery_triggers.rs"]
mod triggers;

pub(super) struct AptWorkflow {
    pub(super) ir: WorkflowIr,
    pub(super) source_helpers: Vec<CompiledSourceHelper>,
    pub(super) pages_approvals: Vec<NativePagesApproval>,
}

/// Compile domain-owned operations and compose a neutral workflow graph.
pub(super) fn workflow(
    config: &AptDeliveryConfig,
    context: &AptRenderContext,
) -> Result<AptWorkflow, RenderError> {
    config
        .validate(".velnor/config.toml")
        .map_err(RenderError::Contract)?;
    context.validate()?;
    let operations = records::records(config, context)?;
    let mut source_helpers = operations.registry();
    let condition = triggers::publication_condition(&config.consumer_repository, &config.branch);
    let mut graph = BTreeMap::new();
    graph.insert(
        "verify".to_owned(),
        tools::prepare(
            jobs::verify(context, &operations)?,
            context,
            true,
            &mut source_helpers,
        )?,
    );
    graph.insert(
        "admit".to_owned(),
        tools::prepare(
            jobs::admit(config, context, &operations)?,
            context,
            false,
            &mut source_helpers,
        )?,
    );
    graph.insert(
        "stage".to_owned(),
        tools::prepare(
            jobs::stage(context, &operations, &condition)?,
            context,
            false,
            &mut source_helpers,
        )?,
    );
    let (deployment, approval) = pages::deploy(config, context, &operations, &mut source_helpers)?;
    graph.insert("deploy".to_owned(), deployment);
    graph.insert(
        "feed-result".to_owned(),
        jobs::result(context, &operations)?,
    );
    let ir = WorkflowIr {
        cache_mode: velnor_actions_contract::CacheMode::Read,
        name: "Package feed".to_owned(),
        run_name: None,
        triggers: triggers::triggers(&config.schedule),
        permissions: Permissions {
            contents: PermissionLevel::Read,
            actions: PermissionLevel::None,
            ..Permissions::default()
        },
        concurrency: Concurrency {
            group: "package-feed-apt-${{ github.repository }}".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs: graph,
    };
    ir.validate().map_err(RenderError::Contract)?;
    Ok(AptWorkflow {
        ir,
        source_helpers,
        pages_approvals: vec![approval],
    })
}

#[cfg(test)]
#[path = "apt_delivery_workflow_tests.rs"]
mod tests;
