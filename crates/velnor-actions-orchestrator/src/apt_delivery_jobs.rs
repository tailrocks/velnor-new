//! Native source operations retain their narrow job authority.
use super::{AptRenderContext, records::AptRecords, steps, triggers};
use velnor_actions_contract::config::AptDeliveryConfig;
use velnor_actions_contract::workflow::{PermissionLevel, Permissions};
use velnor_actions_contract::{CompiledSourceHelper, Job, Step};
use velnor_actions_workflow_renderer::{RenderError, source_helper::source_helper_step};

pub(super) fn helper(name: &str, record: &CompiledSourceHelper) -> Result<Step, RenderError> {
    source_helper_step(name, record, record.environment().clone())
}

pub(super) fn verify(context: &AptRenderContext, records: &AptRecords) -> Result<Job, RenderError> {
    let mut job = steps::job(
        "Verify apt feed",
        &context.workflow.runs_on,
        30,
        vec![
            steps::checkout(),
            helper("Fetch and verify feed inputs", &records.verify)?,
            steps::upload(
                "incoming_upload",
                "apt-incoming-${{ github.run_id }}-${{ github.run_attempt }}",
                "incoming",
            )?,
        ],
    )?;
    job.outputs = steps::artifact_outputs("incoming_upload")?;
    Ok(job)
}

pub(super) fn admit(
    config: &AptDeliveryConfig,
    context: &AptRenderContext,
    records: &AptRecords,
) -> Result<Job, RenderError> {
    let mut job = steps::job(
        "Admit apt publication",
        &context.workflow.runs_on,
        30,
        vec![
            steps::checkout(),
            helper("Admit approved publication source", &records.admit)?,
        ],
    )?;
    job.condition = Some(triggers::admission_condition(
        &config.consumer_repository,
        &config.branch,
    ));
    job.permissions = Some(read_permissions());
    Ok(job)
}

pub(super) fn stage(
    context: &AptRenderContext,
    records: &AptRecords,
    condition: &str,
) -> Result<Job, RenderError> {
    let mut job = steps::job(
        "Stage apt feed",
        &context.workflow.runs_on,
        30,
        vec![
            steps::checkout(),
            helper("Verify and download feed artifact", &records.transport)?,
            helper("Reverify and stage the signed feed", &records.stage)?,
            steps::upload(
                "staging_upload",
                "apt-staging-${{ github.run_id }}-${{ github.run_attempt }}",
                "public",
            )?,
        ],
    )?;
    job.needs = ["verify", "admit"].map(str::to_owned).to_vec();
    job.condition = Some(condition.to_owned());
    job.environment = Some("package-feed".to_owned());
    job.permissions = Some(read_permissions());
    job.outputs = steps::artifact_outputs("staging_upload")?;
    Ok(job)
}

pub(super) fn result(context: &AptRenderContext, records: &AptRecords) -> Result<Job, RenderError> {
    let mut job = steps::job(
        "Feed result",
        &context.workflow.runs_on,
        5,
        vec![helper("Check required feed outcomes", &records.result)?],
    )?;
    job.needs = ["verify", "admit", "stage", "deploy"]
        .map(str::to_owned)
        .to_vec();
    job.condition = Some("${{ always() }}".to_owned());
    Ok(job)
}

pub(super) fn read_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        actions: PermissionLevel::Read,
        ..Permissions::default()
    }
}
