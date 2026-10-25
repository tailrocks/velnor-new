use super::{RepositoryTarget, WorkflowRunError};

pub(super) fn repository_parts(
    expected: RepositoryTarget<'_>,
) -> Result<(&str, &str), WorkflowRunError> {
    if expected.id <= 0 || !path_part(expected.owner) || !path_part(expected.name) {
        return Err(WorkflowRunError::InvalidRequest);
    }
    Ok((expected.owner, expected.name))
}

pub(super) fn parse_pull_request_number(reference: &str) -> Option<i64> {
    reference
        .strip_prefix("refs/pull/")?
        .strip_suffix("/merge")?
        .parse::<i64>()
        .ok()
        .filter(|number| *number > 0)
}

pub(super) fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_nonce(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn path_part(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
