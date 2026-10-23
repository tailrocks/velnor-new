use super::super::identity;
use super::{
    Admission, MAX_ADMISSION_BYTES, admitted_policy_plan, parse_package_release_policy,
    read_source_admission, required,
    source_files::{self, SourceFileEntry},
};
use crate::{OrchestratorError, internal::internal};
use std::path::Path;
use velnor_actions_contract::{
    PackageReleasePlan, config::QualifiedTool, preview_production::ReleaseProductionContext,
};
use velnor_actions_mise::CheckDeadline;

pub(crate) fn validate_admitted_source_checkout(
    temp: &Path,
    workspace: &Path,
    release: &velnor_actions_contract::config::RustBinaryReleaseConfig,
    qualified_tools: &[QualifiedTool],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let bytes = read_source_admission(&temp.join("release-policy-admission.json"))?;
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, MAX_ADMISSION_BYTES)
            .map_err(|_| internal("release_policy_admission_json"))?;
    let admission: Admission =
        serde_json::from_value(value).map_err(|_| internal("release_policy_admission_schema"))?;
    let policy = parse_package_release_policy(required("VELNOR_RELEASE_POLICY")?.as_bytes())
        .map_err(|_| internal("release_policy_invalid"))?;
    let (_, plan) = admitted_policy_plan(temp, deadline)?;
    let mut required_paths = std::collections::BTreeSet::new();
    for path in policy.manifest_paths() {
        required_paths.insert(path.to_owned());
    }
    let discovered = velnor_actions_mise::discover_preview_producer(
        workspace,
        &release.producer,
        qualified_tools,
        deadline,
    )
    .map_err(|_| internal("preview_source_task_discovery"))?;
    required_paths.insert(".velnor/config.toml".to_owned());
    for path in &discovered.config_inputs {
        required_paths.insert(path.clone());
    }
    let actual_paths = admission_paths(&admission.source_files)?;
    if !admission_covers_required_paths(&required_paths, &actual_paths)
        || admission.producer != release.producer
        || admission.task_config != discovered.task_config
        || admission.task_inputs != discovered.config_inputs
        || admission.tool_specs != discovered.tool_specs
        || admission.qualified_tools != discovered.qualified_tools
        || admission.qualification_digest != discovered.qualification_digest
        || admission.context.source_sha != required("VELNOR_RELEASE_SOURCE_SHA")?
        || admission.context.workflow_authority_sha
            != required("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA")?
        || !admitted_preview_version_matches(&plan, &admission.context)
    {
        return Err(internal("release_policy_source_closure_substitution"));
    }
    for entry in &admission.source_files {
        validate_source_file_entry(workspace, entry, deadline)?;
    }
    Ok(())
}

fn validate_source_file_entry(
    workspace: &Path,
    entry: &SourceFileEntry,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if entry.path.starts_with('/')
        || entry
            .path
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        || entry.path.contains('\\')
        || !matches!(entry.mode.as_str(), "100644" | "100755")
        || !identity::is_git_sha(&entry.sha)
    {
        return Err(internal("release_policy_source_closure_entry"));
    }
    let metadata = std::fs::symlink_metadata(workspace.join(&entry.path))
        .map_err(|_| internal("release_policy_source_closure_file"))?;
    if !metadata.file_type().is_file() {
        return Err(internal("release_policy_source_closure_file"));
    }
    #[cfg(unix)]
    verify_source_file_mode(&metadata, entry)?;
    let local = crate::safe_read::read_repo_file(workspace, &entry.path, 1024 * 1024)?;
    let crate::safe_read::RepoRead::Text(local) = local else {
        return Err(internal("release_policy_source_closure_file"));
    };
    if source_files::git_blob_sha1(local.as_bytes())? != entry.sha {
        return Err(internal("release_policy_source_closure_blob"));
    }
    verify_source_git_blob(workspace, entry, deadline)
}

#[cfg(unix)]
fn verify_source_file_mode(
    metadata: &std::fs::Metadata,
    entry: &SourceFileEntry,
) -> Result<(), OrchestratorError> {
    use std::os::unix::fs::PermissionsExt as _;
    let executable = metadata.permissions().mode() & 0o111 != 0;
    if executable != (entry.mode == "100755") {
        return Err(internal("release_policy_source_closure_mode"));
    }
    Ok(())
}

fn verify_source_git_blob(
    workspace: &Path,
    entry: &SourceFileEntry,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
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
    Ok(())
}

pub(super) fn admission_paths(
    files: &[SourceFileEntry],
) -> Result<std::collections::BTreeSet<String>, OrchestratorError> {
    let paths = files
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if paths.len() != files.len() {
        return Err(internal("release_policy_source_closure_duplicate"));
    }
    Ok(paths)
}

pub(super) fn admission_covers_required_paths(
    required: &std::collections::BTreeSet<String>,
    retained: &std::collections::BTreeSet<String>,
) -> bool {
    required.is_subset(retained)
}

pub(super) fn admitted_preview_version_matches(
    plan: &PackageReleasePlan,
    context: &ReleaseProductionContext,
) -> bool {
    velnor_actions_contract::preview_identity::PreviewIdentity::derive(
        &plan.base_version,
        &context.source_sha,
        context.commit_count,
    )
    .is_ok_and(|identity| identity.version == context.version)
}
