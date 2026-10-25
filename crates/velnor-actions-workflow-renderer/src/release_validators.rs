use crate::RenderError;

pub(crate) fn is_clean_text(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && !value.contains("${{")
        && !value.chars().any(char::is_control)
}

pub fn validate_environment(name: &str) -> Result<(), RenderError> {
    let charset = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'.');
    if is_clean_text(name, 128) && name.bytes().all(charset) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_environment:{name}"
        )))
    }
}

fn is_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub fn validate_repository(repository: &str) -> Result<(), RenderError> {
    let invalid = RenderError::InvalidWorkflow(format!("bad_repository:{repository}"));
    if !is_clean_text(repository, 201) || repository.contains(char::is_whitespace) {
        return Err(invalid);
    }
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(invalid);
    };
    if name.contains('/') || !is_repo_segment(owner) || !is_repo_segment(name) {
        return Err(invalid);
    }
    Ok(())
}

pub fn validate_source_sha(sha: &str) -> Result<(), RenderError> {
    let hex = |b: u8| b.is_ascii_digit() || matches!(b, b'a'..=b'f');
    if sha.len() == 40 && sha.bytes().all(hex) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_source_sha:{sha}"
        )))
    }
}

pub fn validate_plan_id(id: &str) -> Result<(), RenderError> {
    let charset = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-');
    if is_clean_text(id, 128) && id.bytes().all(charset) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!("bad_plan_id:{id}")))
    }
}

pub fn validate_package_name(name: &str) -> Result<(), RenderError> {
    let mut bytes = name.bytes();
    let leading = bytes.next().is_some_and(|b| b.is_ascii_alphanumeric());
    let rest = bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'));
    if leading && rest && name.len() <= 64 {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_package_name:{name}"
        )))
    }
}

fn is_version_tail(tail: &str) -> bool {
    !tail.is_empty()
        && tail.len() <= 64
        && tail.split('.').all(|ident| {
            !ident.is_empty()
                && ident
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

pub fn validate_package_version(version: &str) -> Result<(), RenderError> {
    let invalid = RenderError::InvalidWorkflow(format!("bad_package_version:{version}"));
    if !is_clean_text(version, 128) {
        return Err(invalid);
    }
    let head = match version.split_once('+') {
        Some((head, build)) if !build.contains('+') && is_version_tail(build) => head,
        Some(_) => return Err(invalid),
        None => version,
    };
    let core = match head.split_once('-') {
        Some((core, pre)) if is_version_tail(pre) => core,
        Some(_) => return Err(invalid),
        None => head,
    };
    let numeric: Vec<&str> = core.split('.').collect();
    let shaped = numeric.len() == 3
        && numeric
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if shaped { Ok(()) } else { Err(invalid) }
}
