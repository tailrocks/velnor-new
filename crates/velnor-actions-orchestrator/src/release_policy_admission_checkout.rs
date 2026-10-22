use super::source_files;
use crate::OrchestratorError;
use crate::internal::internal;
use std::path::Path;
use velnor_actions_mise::CheckDeadline;

pub(super) fn verify_admitted_source_entries(
    workspace: &Path,
    entries: &[source_files::SourceFileEntry],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    for entry in entries {
        if entry.path.starts_with('/')
            || entry
                .path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || entry.path.contains('\\')
            || !matches!(entry.mode.as_str(), "100644" | "100755")
            || !super::super::identity::is_git_sha(&entry.sha)
        {
            return Err(internal("release_policy_source_closure_entry"));
        }
        let metadata = std::fs::symlink_metadata(workspace.join(&entry.path))
            .map_err(|_| internal("release_policy_source_closure_file"))?;
        if !metadata.file_type().is_file() {
            return Err(internal("release_policy_source_closure_file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let executable = metadata.permissions().mode() & 0o111 != 0;
            if executable != (entry.mode == "100755") {
                return Err(internal("release_policy_source_closure_mode"));
            }
        }
        let local = crate::safe_read::read_repo_file(workspace, &entry.path, 1024 * 1024)?;
        let crate::safe_read::RepoRead::Text(local) = local else {
            return Err(internal("release_policy_source_closure_file"));
        };
        if source_files::git_blob_sha1(local.as_bytes())? != entry.sha {
            return Err(internal("release_policy_source_closure_blob"));
        }
        let object =
            velnor_actions_mise::GitRequest::rev_parse(vec![format!("HEAD:{}", entry.path).into()])
                .run_in_until(workspace, deadline)
                .map_err(|_| internal("release_policy_source_closure_blob"))?;
        if !object.success
            || object
                .stdout_text("git")
                .map_err(|_| internal("release_policy_source_closure_blob"))?
                .trim()
                != entry.sha
        {
            return Err(internal("release_policy_source_closure_blob"));
        }
    }
    Ok(())
}
