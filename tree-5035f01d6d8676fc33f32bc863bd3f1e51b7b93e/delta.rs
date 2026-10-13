//! Source-delta proof derived from the checked-out Git history.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{QualificationSourceDelta, canonical_json_bytes, digest_b3};
use velnor_actions_mise::GitRequest;

use crate::OrchestratorError;
use crate::internal::internal;

const MAX_CHANGED_PATHS: usize = 256;
const MAX_DIFF_BYTES: usize = 1_048_576;

pub(super) fn derive(
    root: &Path,
    base: &str,
    source: &str,
) -> Result<QualificationSourceDelta, OrchestratorError> {
    validate_sha(base)?;
    validate_sha(source)?;
    prove_ancestor(root, base, source)?;
    let paths = changed_paths(root, base, source)?;
    if paths.is_empty() || paths.len() > MAX_CHANGED_PATHS {
        return Err(internal("qualification_source_delta_path_count"));
    }
    let diff_digest =
        digest_b3(&canonical_json_bytes(&paths).map_err(crate::internal::internal_contract)?);
    Ok(QualificationSourceDelta {
        base_source_sha: base.to_owned(),
        source_sha: source.to_owned(),
        changed_paths: paths,
        diff_digest,
        base_is_ancestor: true,
    })
}

fn prove_ancestor(root: &Path, base: &str, source: &str) -> Result<(), OrchestratorError> {
    let request = GitRequest::merge_base(vec![
        OsString::from("--is-ancestor"),
        OsString::from(base),
        OsString::from(source),
    ]);
    let output = request
        .run_in(root)
        .map_err(|_| internal("qualification_git"))?;
    if output.success {
        Ok(())
    } else {
        Err(internal("qualification_source_not_ancestor"))
    }
}

fn changed_paths(root: &Path, base: &str, source: &str) -> Result<Vec<String>, OrchestratorError> {
    let request = GitRequest::diff(vec![
        OsString::from("--name-only"),
        OsString::from("-z"),
        OsString::from("--no-renames"),
        OsString::from(base),
        OsString::from(source),
        OsString::from("--"),
    ]);
    let output = request
        .run_in(root)
        .map_err(|_| internal("qualification_git"))?;
    if !output.success || output.stdout.len() > MAX_DIFF_BYTES {
        return Err(internal("qualification_diff_unavailable"));
    }
    let mut paths: Vec<String> = crate::git_paths::split_nul_paths(&output.stdout)
        .map_err(|_| internal("qualification_diff_non_utf8"))?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn validate_sha(value: &str) -> Result<(), OrchestratorError> {
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(internal("qualification_source_sha_invalid"))
    }
}
