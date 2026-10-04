//! Isolated pinned tooling for the fixed APT delivery jobs.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, Job, SourceBoundOperation, Step, ToolCacheDomain,
};
use velnor_actions_workflow_renderer::{
    RenderError, mise_setup_step, source_helper::source_helper_step,
};

use super::AptRenderContext;
use super::steps::action;

/// Install pinned tools after checkout and prepend typed setup steps.
pub(super) fn prepare(
    mut job: Job,
    context: &AptRenderContext,
    buildx: bool,
    source_helpers: &mut Vec<CompiledSourceHelper>,
) -> Result<Job, RenderError> {
    let Some(checkout) = job.steps.first().cloned() else {
        return Err(RenderError::InvalidWorkflow(
            "apt_job_missing_checkout".to_owned(),
        ));
    };
    let mut original = std::mem::take(&mut job.steps).into_iter();
    let _ = original.next();

    let domain = ToolCacheDomain::Full;
    let bootstrap = context.tools.mise.bootstrap(domain, &job.runs_on)?;
    if !source_helpers.contains(&bootstrap.helper) {
        source_helpers.push(bootstrap.helper.clone());
    }
    let setup = mise_setup_step(&context.tools.mise, domain, &job.runs_on)?;
    let (install, installation) = install_step(context)?;
    if !source_helpers.contains(&installation) {
        source_helpers.push(installation);
    }

    let mut steps = vec![checkout, setup, install];
    if buildx {
        steps.push(action(
            "Set up Buildx",
            &context.buildx_action,
            BTreeMap::from([
                ("cache-binary".to_owned(), "false".to_owned()),
                (
                    "driver-opts".to_owned(),
                    format!("image={}", context.buildkit_image),
                ),
                ("version".to_owned(), context.buildx_version.clone()),
            ]),
        ));
    }
    steps.extend(original);
    job.steps = steps;
    Ok(job)
}

/// Build the exact installed-tool argv and owned execution environment.
fn install_step(context: &AptRenderContext) -> Result<(Step, CompiledSourceHelper), RenderError> {
    context.tools.validate()?;
    let installation = context.tools.preparation.clone();
    if installation.invocation().descriptor().operation() != SourceBoundOperation::MiseToolPrepare {
        return Err(RenderError::InvalidWorkflow(
            "apt_delivery_preparation_operation".to_owned(),
        ));
    }
    let step = source_helper_step(
        "Install exact delivery tools",
        &installation,
        installation.environment().clone(),
    )?;
    Ok((step, installation))
}
