//! Structural admission graph fixtures only; no distribution or runtime proof.
#![cfg(test)]

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledNativeExecRecipe, CompiledSourceHelper, HelperInvocation, NativeCredentialScope,
    SourceBoundHelper, SourceBoundOperation, ToolCacheDomain, compiled_source_sha256,
    generated_source,
};
use velnor_actions_workflow_renderer::RenderError;

/// Model Full Python execution and a distinct Planning Gh installation.
pub(super) fn tools(
    context: &super::AptRenderContext,
) -> Result<(CompiledNativeExecRecipe, Vec<CompiledSourceHelper>), RenderError> {
    let python = format!("python@{}", context.tools.python_version);
    let gh = format!("gh@{}", context.tools.gh_version);
    let mut environment = context.tools.isolation_env_pairs();
    environment.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
    environment.insert(
        "MISE_DATA_DIR".to_owned(),
        ToolCacheDomain::Full.root().to_owned(),
    );
    environment.insert(
        "VELNOR_ADMISSION_PLANNING_GH".to_owned(),
        "/fixture-only/planning/gh".to_owned(),
    );
    let mut prefix = [
        "/usr/bin/env",
        "-i",
        "/fixture-only/mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
    ]
    .map(str::to_owned)
    .to_vec();
    prefix.extend([python.clone(), "--".to_owned()]);
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        prefix,
        environment,
        vec![python.clone()],
        NativeCredentialScope::GithubReadOnly,
    )
    .map_err(RenderError::Contract)?;
    let full = context
        .tools
        .mise
        .bootstrap(ToolCacheDomain::Full, &context.workflow.runs_on)?;
    let planning = context
        .tools
        .mise
        .bootstrap(ToolCacheDomain::Planning, &context.workflow.runs_on)?;
    let records = vec![
        full.helper.clone(),
        planning.helper.clone(),
        preparation(context, ToolCacheDomain::Full, python)?,
        preparation(context, ToolCacheDomain::Planning, gh)?,
    ];
    Ok((recipe, records))
}

/// Neutral no-op records describe independent selector footprints in test graphs.
fn preparation(
    context: &super::AptRenderContext,
    domain: ToolCacheDomain,
    selector: String,
) -> Result<CompiledSourceHelper, RenderError> {
    let source = generated_source(
        &context.workflow.generator_version,
        "# Fixture-only neutral preparation; no tool acquisition or qualification.\nexit 0\n",
    )
    .map_err(RenderError::Contract)?;
    let operation = SourceBoundOperation::MiseToolPrepare;
    let descriptor = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &compiled_source_sha256(source.as_bytes()),
    )
    .map_err(RenderError::Contract)?;
    let invocation = HelperInvocation::compiled(descriptor, Vec::new(), vec![selector])
        .map_err(RenderError::Contract)?;
    let environment = BTreeMap::from([("MISE_DATA_DIR".to_owned(), domain.root().to_owned())]);
    CompiledSourceHelper::compiled(invocation, source)
        .map(|helper| helper.with_environment(environment))
        .map_err(RenderError::Contract)
}
