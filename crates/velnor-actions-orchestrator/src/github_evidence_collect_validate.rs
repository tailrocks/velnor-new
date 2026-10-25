use super::Invocation;
use super::api::{EvidenceResult, WorkflowAttemptDetail, WorkflowRunResponse};
use crate::internal::internal;
use velnor_actions_contract::{
    is_valid_github_evidence_conclusion, is_valid_github_evidence_status,
};

pub(super) fn validate_run(
    run: &WorkflowRunResponse,
    invocation: &Invocation,
    workflow_id: u64,
    in_window: bool,
) -> EvidenceResult<()> {
    let bounds_valid = if in_window {
        run.created_at >= invocation.window.start_text()
            && run.created_at < invocation.window.end_text()
    } else {
        run.created_at < invocation.window.start_text()
    };
    let repo_name = format!(
        "{}/{}",
        invocation.repository.owner, invocation.repository.name
    );
    if run.id == 0
        || run.workflow_id != workflow_id
        || run.run_attempt == 0
        || run.event != "push"
        || run.head_branch != invocation.spec.default_branch
        || !lower_hex(&run.head_sha, 40)
        || !is_valid_github_evidence_status(&run.status)
        || !is_valid_github_evidence_conclusion(run.conclusion.as_deref())
        || !valid_timestamp(&run.created_at)
        || !bounds_valid
        || run.repository.id != invocation.repository_id
        || run.head_repository.as_ref().is_none_or(|repository| {
            repository.id != invocation.repository_id
                || !repository.full_name.eq_ignore_ascii_case(&repo_name)
        })
        || !run.repository.full_name.eq_ignore_ascii_case(&repo_name)
    {
        return Err(internal("github_evidence_run_identity"));
    }
    Ok(())
}

pub(super) fn validate_attempt(
    attempt: &WorkflowAttemptDetail,
    summary: &WorkflowRunResponse,
    workflow_id: u64,
    expected_attempt: u32,
) -> EvidenceResult<()> {
    if attempt.id != summary.id
        || attempt.workflow_id != workflow_id
        || attempt.run_attempt != expected_attempt
        || attempt.event != "push"
        || attempt.head_branch != summary.head_branch
        || attempt.head_sha != summary.head_sha
        || attempt.created_at != summary.created_at
        || !lower_hex(&attempt.head_sha, 40)
        || !valid_timestamp(&attempt.created_at)
        || !is_valid_github_evidence_status(&attempt.status)
        || !is_valid_github_evidence_conclusion(attempt.conclusion.as_deref())
    {
        return Err(internal("github_evidence_attempt_identity"));
    }
    Ok(())
}

pub(super) fn workflow_slug(path: &str) -> EvidenceResult<&str> {
    path.strip_prefix(".github/workflows/")
        .and_then(|value| value.strip_suffix(".yml"))
        .ok_or_else(|| internal("github_evidence_workflow_path"))
}

fn lower_hex(value: &str, expected: usize) -> bool {
    value.len() == expected
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        })
}
