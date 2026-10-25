use velnor_actions_contract::WorkflowEvent;

pub(super) fn fork_matches(
    event: WorkflowEvent,
    fork: Option<bool>,
    head_repository: Option<&str>,
    head_repository_id: Option<u64>,
    base_repository: Option<&str>,
    base_repository_id: Option<u64>,
) -> bool {
    match event {
        WorkflowEvent::PullRequest => fork == Some(false),
        WorkflowEvent::Fork => {
            fork == Some(true)
                && head_repository != base_repository
                && head_repository_id != base_repository_id
        }
        _ => false,
    }
}

pub(super) fn merge_parents_match(parents: &[String], base_sha: &str, head_sha: &str) -> bool {
    parents.len() == 2 && parents[0] == base_sha && parents[1] == head_sha
}

pub(super) fn valid_sha(value: Option<&str>) -> bool {
    value.is_some_and(|sha| {
        sha.len() == 40
            && sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
