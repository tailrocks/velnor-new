//! Native APT authority reconstructed from source and SDK owners.
use super::AptRenderContext;
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{
    CompiledSourceHelper, CompiledSupportSource, config::AptDeliveryConfig,
};
use velnor_actions_native::apt::{self, AptOperation};
use velnor_actions_workflow_renderer::RenderError;
#[cfg(not(test))]
#[path = "apt_delivery_admission.rs"]
mod admission;
#[cfg(test)]
#[path = "apt_delivery_admission_fixture.rs"]
mod admission_fixture;
#[path = "apt_delivery_environments.rs"]
mod environments;
#[path = "apt_delivery_execution.rs"]
mod execution;

pub(super) struct AptRecords {
    pub(super) verify: CompiledSourceHelper,
    pub(super) admit: CompiledSourceHelper,
    pub(super) transport: CompiledSourceHelper,
    pub(super) stage: CompiledSourceHelper,
    pub(super) pages: CompiledSourceHelper,
    pub(super) result: CompiledSourceHelper,
    pub(super) admission_tools: Vec<CompiledSourceHelper>,
}

impl AptRecords {
    pub(super) fn registry(&self) -> Vec<CompiledSourceHelper> {
        let mut records = vec![
            self.verify.clone(),
            self.admit.clone(),
            self.transport.clone(),
            self.stage.clone(),
            self.pages.clone(),
            self.result.clone(),
        ];
        records.extend(self.admission_tools.iter().cloned());
        records
    }
}

pub(super) fn records(
    config: &AptDeliveryConfig,
    context: &AptRenderContext,
) -> Result<AptRecords, RenderError> {
    let read = execution::recipe(context, NativeCredentialScope::GithubReadOnly)?;
    let (admit, admission_tools) = admission_record(config, context)?;
    let admission = CompiledSupportSource::compiled(
        crate::release_emit::release_admission::ADMISSION_PATH,
        crate::release_emit::release_admission::source(),
        &context.workflow.generator_version,
    )
    .map_err(RenderError::Contract)?;
    let verify = operation(AptOperation::Verify, config, context)?
        .with_execution_recipe(read.clone())
        .map_err(RenderError::Contract)?;
    let transport = operation(AptOperation::IncomingTransport, config, context)?
        .with_execution_recipe(read.clone())
        .map_err(RenderError::Contract)?;
    let signing = execution::recipe(context, NativeCredentialScope::AptSigning)?;
    let stage = operation(AptOperation::Stage, config, context)?
        .with_execution_recipe(signing)
        .map_err(RenderError::Contract)?;
    let pages = apt::compiled_pages_admission(
        config,
        &context.workflow.generator_version,
        &admission,
        environments::environment(AptOperation::PagesAdmission, config),
    )
    .map_err(RenderError::Contract)?
    .with_execution_recipe(read)
    .map_err(RenderError::Contract)?;
    let result = operation(AptOperation::Result, config, context)?;
    Ok(AptRecords {
        verify,
        admit,
        transport,
        stage,
        pages,
        result,
        admission_tools,
    })
}

fn operation(
    operation: AptOperation,
    config: &AptDeliveryConfig,
    context: &AptRenderContext,
) -> Result<CompiledSourceHelper, RenderError> {
    apt::compiled_helper(
        operation,
        config,
        &context.workflow.generator_version,
        environments::environment(operation, config),
    )
    .map_err(RenderError::Contract)
}

fn admission_record(
    config: &AptDeliveryConfig,
    context: &AptRenderContext,
) -> Result<(CompiledSourceHelper, Vec<CompiledSourceHelper>), RenderError> {
    #[cfg(test)]
    {
        let (recipe, tools) = admission_fixture::tools(context)?;
        let helper = crate::release_emit::release_admission::fixture_default_branch_admission(
            &config.consumer_repository,
            &config.branch,
            &context.workflow.generator_version,
            recipe,
        )?;
        Ok((helper, tools))
    }
    #[cfg(not(test))]
    {
        admission::record(config, context)
    }
}
