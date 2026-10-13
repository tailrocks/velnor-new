/// Check the `r<digits>-a<digits>` CI run-key shape.
fn is_ci_run_key(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('r') else {
        return false;
    };
    let Some((run, attempt)) = rest.split_once("-a") else {
        return false;
    };
    !run.is_empty()
        && !attempt.is_empty()
        && run.bytes().all(|b| b.is_ascii_digit())
        && attempt.bytes().all(|b| b.is_ascii_digit())
}

/// Validate one lowercase component name (`.`/`..` are never components).
fn validate_component(value: &str, field: &'static str) -> Result<(), ContractError> {
    if !value.is_empty() && value != "." && value != ".." && value.bytes().all(is_component_byte) {
        Ok(())
    } else {
        Err(ContractError::identity(field, "bad_component"))
    }
}

/// Validate `/`-separated manifest-key segments.
fn validate_path_segments(path: &str, field: &'static str) -> Result<(), ContractError> {
    if path.is_empty() {
        return Err(ContractError::identity(field, "empty_path"));
    }
    for segment in path.split('/') {
        validate_component(segment, field)?;
    }
    Ok(())
}

/// Validate a fetch workspace root for shell interpolation.
///
/// Single source shared by the fetch-step generator (orchestrator) and
/// the render-time ambient-auth exemption (renderer): both agree on
/// which roots are safe by construction. Empty is the root workspace.
/// Rejects parent traversal, quoting and expansion characters,
/// backslashes, newlines, and absolute paths.
/// # Errors
pub fn validate_fetch_root(root: &str) -> Result<(), ContractError> {
    let bad = root.contains("..")
        || root.contains('\'')
        || root.contains('"')
        || root.contains('$')
        || root.contains('`')
        || root.contains('\\')
        || root.contains('\n')
        || root.starts_with('/');
    if bad {
        return Err(ContractError::identity(
            "fetch.root",
            format!("unsafe_fetch_root:{root}"),
        ));
    }
    Ok(())
}
