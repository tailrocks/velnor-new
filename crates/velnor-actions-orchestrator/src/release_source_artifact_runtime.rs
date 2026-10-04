//! Runtime input code emitted only from a freshly reconstructed source transport binding.
use super::{CompiledSourceArtifactInput, OrchestratorError, invalid};

pub(super) fn source(input: &CompiledSourceArtifactInput) -> Result<String, OrchestratorError> {
    let identity = serde_json::to_string(&[
        input.repository.as_str(),
        input.source_sha.as_str(),
        input.branch.as_str(),
        input.source_owner.invocation().descriptor().source_sha256(),
    ])
    .map_err(|_| invalid("compiled_input_encoding"))?;
    Ok(format!("_SOURCE_INPUT_OWNER_IDENTITY = {identity}\n{BODY}"))
}

const BODY: &str = r#"
def _compiled_source_artifact_input():
    # This function is emitted by the actual source-owner factory and bound to
    # the enclosing compiled helper's exact environment and artifact download.
    # It is never accepted as a user descriptor or imported from the repository.
    repository, source_sha, branch, helper_sha256 = _SOURCE_INPUT_OWNER_IDENTITY
    require(os.environ.get('RELEASE_SOURCE_COMMIT_SHA') == source_sha,
            'source_artifact_compiled_source')
    expected_ref = 'refs/heads/' + branch
    require(os.environ.get('GITHUB_REF') == expected_ref and
            os.environ.get('GITHUB_WORKFLOW_REF') ==
            repository + '/.github/workflows/release.yml@' + expected_ref,
            'source_artifact_compiled_workflow')
    runner_temp = Path(os.environ['RUNNER_TEMP'])
    require(runner_temp.is_absolute() and runner_temp.resolve(strict=True) == runner_temp,
            'source_artifact_compiled_runner_temp')
    binding = {
        'repository': repository, 'source_sha': source_sha,
        'tree_sha': os.environ['RELEASE_SOURCE_TREE_SHA'],
        'workflow': '.github/workflows/release.yml',
        'workflow_sha': os.environ['GITHUB_WORKFLOW_SHA'], 'ref': expected_ref,
        'run_id': os.environ['GITHUB_RUN_ID'], 'attempt': os.environ['GITHUB_RUN_ATTEMPT'],
        'producer_job': 'release-source-snapshot',
        'producer_helper_sha256': helper_sha256,
        'artifact_id': os.environ['RELEASE_SOURCE_SNAPSHOT_ARTIFACT_ID'],
        'raw_zip_sha256': os.environ['RELEASE_SOURCE_SNAPSHOT_ARTIFACT_DIGEST'],
        'inner_sha256': os.environ['RELEASE_SOURCE_SNAPSHOT_BLOB_SHA256'],
        'destination': str(runner_temp / 'velnor' / 'release-source-input'),
    }
    _source_artifact_content_binding(binding)
    return binding
"#;
