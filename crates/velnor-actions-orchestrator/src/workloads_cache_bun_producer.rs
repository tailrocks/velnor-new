//! Compiled source authority for a pure Bun native download producer.

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    ToolCacheDomain,
};

use crate::{OrchestratorError, workloads::cache_eligibility::NativeNpmSource};

pub(crate) fn executable() -> String {
    format!(
        "{}/installs/bun/{}/bin/bun",
        ToolCacheDomain::BunBootstrap.root(),
        velnor_actions_mise::catalog::BUN_VERSION
    )
}
const PUBLIC_PROOF: &str = include_str!("workloads_cache_npm_proof.py");
const NATIVE_PRODUCER: &str = include_str!("workloads_cache_bun_producer.py");

/// Exact descriptor, full literal tuples, pinned native executable and source bytes.
pub(crate) fn compiled_helper(
    sources: &[NativeNpmSource],
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    if sources.is_empty()
        || sources.len() > 1024
        || !sources
            .iter()
            .all(crate::workloads::cache_eligibility::valid_source_candidate)
    {
        return Err(contract("bun_producer_unqualified_sources"));
    }
    let mut sources = sources.to_vec();
    sources.sort();
    sources.dedup();
    let mut arguments = vec![
        executable(),
        velnor_actions_mise::catalog::BUN_VERSION.to_owned(),
    ];
    for source in sources {
        arguments.push(serde_json::to_string(&source).map_err(contract)?);
    }
    let source =
        velnor_actions_contract::generated_source(version, &producer_body()?).map_err(contract)?;
    let digest = Sha256::digest(source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let operation = SourceBoundOperation::BunSourceProducer;
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).map_err(contract)?;
    let invocation =
        HelperInvocation::compiled(descriptor, arguments, Vec::new()).map_err(contract)?;
    CompiledSourceHelper::compiled(invocation, source).map_err(contract)
}

pub(crate) fn source_schema_digest(version: &str) -> Result<String, OrchestratorError> {
    let source = velnor_actions_contract::generated_source(version, &producer_body()?)?;
    Ok(Sha256::digest(source.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn producer_body() -> Result<String, OrchestratorError> {
    let python = format!(
        "__name__ = 'velnor_public_source_library'\n{PUBLIC_PROOF}\n__name__ = '__main__'\n{NATIVE_PRODUCER}"
    );
    let command = velnor_actions_workflow_renderer::join_argv_for_run(&[
        "/usr/bin/python3".to_owned(),
        "-I".to_owned(),
        "-c".to_owned(),
        python,
    ])?;
    Ok(format!(
        "set -eu\nexec /usr/bin/env -i RUNNER_TEMP=\"$RUNNER_TEMP\" GITHUB_OUTPUT=\"$GITHUB_OUTPUT\" {command} \"$@\""
    ))
}

fn contract(error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
#[path = "workloads_cache_bun_producer_tests.rs"]
mod tests;
