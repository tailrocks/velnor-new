//! Closed SourceIntent records; runtime suppliers and whole-workflow gates stay required.
use super::{GENERATOR_VERSION, JobInputs, base_environment, contract, release_host};
use crate::OrchestratorError;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    generated_source,
};
use velnor_actions_mise::catalog::delivery_tools;

#[path = "release_source_intent_prepared_sources.rs"]
mod prepared;

#[path = "release_prepared_artifact_input.rs"]
pub(super) mod prepared_input;

#[path = "release_source_intent_verify_sources.rs"]
mod verified;

/// Exact owned Full bootstrap and Python preparation records, in execution order.
pub(super) fn tool_records(
    inputs: &JobInputs<'_>,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let host = release_host(inputs.label)?;
    let tools = delivery_tools::source_intent_control_tools(host, GENERATOR_VERSION)
        .map_err(|error| contract(format!("source_intent_control_tools:{error}")))?;
    delivery_tools::validate_source_intent_control_tools(host, GENERATOR_VERSION, &tools)
        .map_err(|error| contract(format!("source_intent_control_tools:{error}")))?;
    Ok(vec![tools.bootstrap().clone(), tools.preparation().clone()])
}

/// Compile the actual selection, source leaf and anonymous interpreter envelope.
pub(super) fn prepared_record(
    inputs: &JobInputs<'_>,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let host = release_host(inputs.label)?;
    let tools = delivery_tools::source_intent_control_tools(host, GENERATOR_VERSION)
        .map_err(|error| contract(format!("source_intent_control_tools:{error}")))?;
    delivery_tools::validate_source_intent_control_tools(host, GENERATOR_VERSION, &tools)
        .map_err(|error| contract(format!("source_intent_control_tools:{error}")))?;
    let compiled = prepared::CompiledPreparedSource::compile(inputs)?;
    let source = generated_source(GENERATOR_VERSION, compiled.body())?;
    let operation = SourceBoundOperation::RustReleasePreparedPackage;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let recipe = tools.execution().clone();
    let invocation = HelperInvocation::compiled(
        descriptor,
        Vec::new(),
        recipe.installed_selectors().to_vec(),
    )?;
    let mut environment = base_environment(inputs, &recipe)?;
    environment.extend(compiled.environment().clone());
    environment.insert("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned());
    CompiledSourceHelper::compiled(invocation, source)?
        .with_environment(environment)
        .with_execution_recipe(recipe)?
        .with_github_output()
        .map_err(Into::into)
}

/// Compile verification against the genuine Prepared leaf and its original bytes.
pub(super) fn verified_record(
    inputs: &JobInputs<'_>,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let host = release_host(inputs.label)?;
    let tools = delivery_tools::source_intent_control_tools(host, GENERATOR_VERSION)
        .map_err(|error| contract(format!("source_intent_control_tools:{error}")))?;
    delivery_tools::validate_source_intent_control_tools(host, GENERATOR_VERSION, &tools)
        .map_err(|error| contract(format!("source_intent_control_tools:{error}")))?;
    let compiled = verified::CompiledVerifiedSource::compile(inputs)?;
    let source = generated_source(GENERATOR_VERSION, compiled.body())?;
    let operation = SourceBoundOperation::RustReleasePackageVerify;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let recipe = tools.execution().clone();
    let invocation = HelperInvocation::compiled(
        descriptor,
        Vec::new(),
        recipe.installed_selectors().to_vec(),
    )?;
    let mut environment = base_environment(inputs, &recipe)?;
    environment.extend(compiled.environment().clone());
    CompiledSourceHelper::compiled(invocation, source)?
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .map_err(Into::into)
}
