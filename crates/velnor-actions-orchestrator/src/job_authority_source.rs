use std::path::Path;

use velnor_actions_contract::WorkflowPolicy;

use crate::OrchestratorError;
use crate::job_authority_inventory::inventory_error;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

const GENERATOR_LOCK_PATH: &str = ".velnor/generator.lock";
const RELEASE_MANIFEST_PATH: &str = ".velnor/release-manifest.json";

pub(super) fn generator_source_sha(
    root: &Path,
    policy: WorkflowPolicy,
) -> Result<String, OrchestratorError> {
    let (path, source_sha) = match policy {
        WorkflowPolicy::VelnorRepositoryV1 => {
            let lock = read_text(root, GENERATOR_LOCK_PATH)?;
            let lock = velnor_actions_mise::catalog::lock::parse_generator_lock(&lock)
                .map_err(|error| inventory_error(&format!("generator_lock_invalid:{error}")))?;
            (GENERATOR_LOCK_PATH, lock.generator.commit)
        }
        WorkflowPolicy::ConsumerV1 => {
            let manifest = read_text(root, RELEASE_MANIFEST_PATH)?;
            let manifest = velnor_actions_mise::catalog::lock::parse_release_manifest(&manifest)
                .map_err(|error| inventory_error(&format!("release_manifest_invalid:{error}")))?;
            (RELEASE_MANIFEST_PATH, manifest.commit)
        }
    };
    if source_sha.len() != 40
        || !source_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(inventory_error(&format!(
            "generator_source_sha_invalid:{path}"
        )));
    }
    Ok(source_sha)
}

fn read_text(root: &Path, path: &str) -> Result<String, OrchestratorError> {
    match read_repo_file(root, path, MAX_REPO_FILE_BYTES)? {
        RepoRead::Text(text) => Ok(text),
        RepoRead::Absent => Err(inventory_error(&format!(
            "generator_identity_missing:{path}"
        ))),
    }
}
