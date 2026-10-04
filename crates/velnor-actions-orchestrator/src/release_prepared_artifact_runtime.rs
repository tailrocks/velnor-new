//! Runtime binding emitted only from the fresh prepared-artifact owner.

use super::{CompiledPreparedArtifactInput, OrchestratorError, invalid};

pub(super) fn source(input: &CompiledPreparedArtifactInput) -> Result<String, OrchestratorError> {
    let identity = serde_json::to_string(&[
        input.repository.as_str(),
        input.source_sha.as_str(),
        input.branch.as_str(),
        input
            .prepared_owner
            .invocation()
            .descriptor()
            .source_sha256(),
        input.policy.as_str(),
    ])
    .map_err(|_| invalid("compiled_input_encoding"))?;
    Ok(format!(
        "_COMPILED_PREPARED_ARTIFACT_IDENTITY = {identity}\n{BODY}"
    ))
}

const BODY: &str = r#"
def _compiled_prepared_artifact_input():
    # This override is emitted after the actual prepared input loader in the
    # same sealed namespace. User dictionaries cannot replace this binding.
    repository, source_sha, branch, helper_sha256, policy_json = \
        _COMPILED_PREPARED_ARTIFACT_IDENTITY
    expected_ref = 'refs/heads/' + branch
    workflow = '.github/workflows/release.yml'
    expected_workflow_ref = repository + '/' + workflow + '@' + expected_ref
    require(os.environ.get('GITHUB_REPOSITORY') == repository,
            'prepared_artifact_compiled_repository')
    require(os.environ.get('RELEASE_SOURCE_COMMIT_SHA') == source_sha,
            'prepared_artifact_compiled_source')
    require(os.environ.get('GITHUB_REF') == expected_ref,
            'prepared_artifact_compiled_ref')
    require(os.environ.get('GITHUB_WORKFLOW_REF') == expected_workflow_ref,
            'prepared_artifact_compiled_workflow')
    workflow_sha = os.environ.get('GITHUB_WORKFLOW_SHA', '')
    require(re.fullmatch(r'[0-9a-f]{40}', workflow_sha) is not None,
            'prepared_artifact_workflow_sha')
    run_id = os.environ.get('GITHUB_RUN_ID', '')
    attempt = os.environ.get('GITHUB_RUN_ATTEMPT', '')
    require(re.fullmatch(r'[1-9][0-9]*', run_id) is not None and
            re.fullmatch(r'[1-9][0-9]*', attempt) is not None,
            'prepared_artifact_run_identity')
    approved = policy()
    require(same_json(approved, decode_json(policy_json)),
            'prepared_artifact_compiled_policy')
    workspace = Path(os.environ.get('GITHUB_WORKSPACE', ''))
    require(workspace.is_absolute() and workspace.resolve(strict=True) == workspace,
            'prepared_artifact_compiled_workspace')
    binding = {
        'approved': approved,
        'repository': repository, 'source_sha': source_sha,
        'workflow': workflow, 'workflow_sha': workflow_sha, 'ref': expected_ref,
        'run_id': run_id, 'attempt': attempt,
        'producer_job': 'release-package',
        'producer_helper_sha256': helper_sha256,
        'artifact_id': os.environ.get('RELEASE_PACKAGE_ARTIFACT_ID', ''),
        'raw_zip_sha256': os.environ.get('RELEASE_PACKAGE_ARTIFACT_DIGEST', ''),
        'inner_sha256': os.environ.get('RELEASE_PACKAGE_BLOB_SHA256', ''),
        'destination': str(workspace / 'release-prepared-input'),
    }
    _prepared_content_binding(binding)
    return binding
"#;
