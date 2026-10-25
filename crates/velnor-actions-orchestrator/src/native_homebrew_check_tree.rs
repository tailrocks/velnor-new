use super::{
    CheckPlatform, NativeCheckIdentityExtension, NativeHomebrewOperation, Path, WorkflowEvent,
};

pub(crate) fn ensure_tracked_tree_clean(
    root: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<(), crate::OrchestratorError> {
    use crate::internal::internal;
    let result =
        velnor_actions_mise::GitRequest::diff(vec!["--quiet".into(), "HEAD".into(), "--".into()])
            .run_in_until(root, deadline)
            .map_err(|error| internal(&error.to_string()))?;
    result
        .require_success("git")
        .map_err(|_| internal("native_check_tracked_tree_dirty"))?;
    Ok(())
}

/// Recheck the exact checkout and every regular-file tree entry at execution or Required.
pub(crate) fn read_checked_tap_tree(
    root: &Path,
    identity: &NativeCheckIdentityExtension,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<Vec<velnor_actions_mise::TapTreeEntry>, crate::OrchestratorError> {
    let head = git_value(root, vec!["HEAD".to_owned()], deadline)?;
    let tree_object = git_value(root, vec!["HEAD^{tree}".to_owned()], deadline)?;
    if head != identity.head || tree_object != identity.tree_object {
        return Err(crate::internal::internal("native_check_checkout_identity"));
    }
    ensure_tracked_tree_clean(root, deadline)?;
    let tree = velnor_actions_mise::read_tap_tree(root, &head, deadline)
        .map_err(|error| crate::internal::internal(&error.to_string()))?;
    if tree.is_empty()
        || tree
            .iter()
            .any(|entry| entry.mode != 0o100_644 && entry.mode != 0o100_755)
    {
        return Err(crate::internal::internal(
            "native_check_tree_nonregular_entry",
        ));
    }
    Ok(tree)
}

pub(crate) fn git_value(
    root: &Path,
    args: Vec<String>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<String, crate::OrchestratorError> {
    use crate::internal::{internal, internal_contract};
    let args = args.into_iter().map(std::ffi::OsString::from).collect();
    let result = velnor_actions_mise::GitRequest::rev_parse(args)
        .run_in_until(root, deadline)
        .map_err(|error| internal(&error.to_string()))?;
    result
        .require_success("git")
        .map_err(|error| internal(&error.to_string()))?;
    let value = result
        .stdout_text("git")
        .map_err(|error| internal(&error.to_string()))?
        .trim()
        .to_owned();
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(internal_contract(
            velnor_actions_contract::ContractError::identity(
                "native_check_git_identity",
                "malformed_git_object_id",
            ),
        ));
    }
    Ok(value)
}

pub(crate) fn pull_request_number_from_env(
    event: WorkflowEvent,
) -> Result<Option<u64>, crate::OrchestratorError> {
    if !matches!(event, WorkflowEvent::PullRequest | WorkflowEvent::Fork) {
        return Ok(None);
    }
    let event_path = std::env::var_os("GITHUB_EVENT_PATH")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| crate::internal::internal("native_check_event_payload_missing"))?;
    let path = Path::new(&event_path);
    let payload = crate::safe_read::read_event_file(path, crate::safe_read::MAX_REPO_FILE_BYTES)?;
    let value: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|_| crate::internal::internal("native_check_event_payload_invalid"))?;
    let number = value
        .pointer("/pull_request/number")
        .and_then(serde_json::Value::as_u64)
        .filter(|number| *number > 0)
        .ok_or_else(|| crate::internal::internal("native_check_pull_request_missing"))?;
    Ok(Some(number))
}

pub(crate) fn tool_profile(operation: NativeHomebrewOperation, platform: CheckPlatform) -> String {
    format!("{}-{}", operation.as_str(), platform.target())
}

/// Independently resolve protected tap policy and require every source-derived
/// native operation in the final Required plan. Candidate declarations can
/// add checks, but cannot erase a protected tree, formula, or install check.
pub(crate) fn caller_repository(root: &Path) -> Result<String, crate::OrchestratorError> {
    use crate::internal::internal;

    let repository = std::env::var(crate::origin::GITHUB_REPOSITORY_ENV)
        .ok()
        .filter(|value| velnor_actions_contract::vcs::is_safe_github_repository(value))
        .ok_or_else(|| internal("native_check_repository_missing"))?;
    let origin = crate::release_identity::origin_repository(root)?;
    if !caller_repository_matches(&repository, &origin) {
        return Err(internal("native_check_repository_origin_mismatch"));
    }
    Ok(repository)
}

pub(crate) fn caller_repository_matches(repository: &str, origin: &str) -> bool {
    let Some(repository) = crate::origin::validate_repository_slug(repository) else {
        return false;
    };
    crate::origin::validate_repository_slug(origin).as_deref() == Some(repository.as_str())
}
