//! Closed ownership prerequisite for an isolated OpenTofu source producer.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation, Step, StepId,
    ToolCacheDomain,
};

use crate::OrchestratorError;

const PRODUCER_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-provider-producer";

pub(crate) fn producer_home(root: &str) -> Result<String, OrchestratorError> {
    velnor_actions_tofu::validate_normalized_root(root)?;
    Ok(format!(
        "{PRODUCER_BASE_EXPR}/{}/home",
        velnor_actions_tofu::tofu_root_locator(root)?
    ))
}

/// Reconstruct the complete prerequisite from its normalized root authority.
pub(crate) fn record(root: &str, version: &str) -> Result<CompiledSourceHelper, OrchestratorError> {
    let home = producer_home(root)?;
    if version != env!("CARGO_PKG_VERSION") {
        return Err(OrchestratorError::Contract {
            problem: "tofu_ownership_version".to_owned(),
        });
    }
    let body = format!(
        "PATH=/usr/bin:/bin\nexport PATH\ntest \"$#\" -eq 1 || exit 1\n{}\nown_directory \"$HOME\"\n",
        include_str!("tofu_root_owner.sh")
    );
    let source = velnor_actions_contract::generated_source(version, &body)?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::TofuRootOwnership;
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let invocation = HelperInvocation::compiled(
        descriptor,
        vec![velnor_actions_tofu::key_for_root(root)],
        Vec::new(),
    )?;
    let mut environment = BTreeMap::from([
        ("HOME".to_owned(), home),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        (
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::TofuBootstrap.root().to_owned(),
        ),
    ]);
    environment.extend(
        velnor_actions_mise::ISOLATION_ENV
            .iter()
            .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV.iter())
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    Ok(CompiledSourceHelper::compiled(invocation, source)?.with_environment(environment))
}

pub(crate) fn step(record: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Bind isolated Tofu root",
        record,
        record.environment().clone(),
    )
    .map_err(OrchestratorError::from)?;
    step.id = Some(StepId::new("velnor-tofu-root-ownership")?);
    Ok(step)
}

#[cfg(test)]
#[path = "tofu_ownership_helper_tests.rs"]
mod tests;
