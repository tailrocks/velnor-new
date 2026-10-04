//! Frozen original-source expectations; no installed service-context or native issuer.
use crate::OrchestratorError;
use crate::release_emit::release_steps::JobInputs;

/// Emit frozen input expectations as part of the owning SourceSnapshot factory.
/// This does not qualify repository context, a SourceJob, or a native source lease.
/// No source helper/job factory is called: the owner may include this same code.
pub(crate) fn context_source(inputs: &JobInputs<'_>) -> Result<String, OrchestratorError> {
    let expected = serde_json::to_string(&[
        inputs.repository,
        inputs.sha,
        inputs.branch,
        "release-source-snapshot",
    ])
    .map_err(|error| OrchestratorError::Contract {
        problem: format!("original_source_context_encoding:{error}"),
    })?;
    Ok(format!(
        "_ORIGINAL_SOURCE_EXPECTED_IDENTITY = {expected}\n{BODY}"
    ))
}

const BODY: &str = r#"
def _compiled_original_source_context():
    # Frozen expected data cannot mint the absent serviceREST/graph/native proof.
    # The original context remains unavailable even in a compiled source closure.
    return None
"#;
