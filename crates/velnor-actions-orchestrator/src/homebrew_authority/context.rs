use super::{
    CheckDeadline, OrchestratorError, WorkflowEvent, admit_pull_request_context, internal,
};
use crate::package_release_publish::MergeGroupSelection;

#[path = "merge_group.rs"]
mod merge_group;

pub(super) struct TapContext {
    pub(super) tap_repository_id: u64,
    pub(super) tap_base_sha: String,
    pub(super) authenticated_head: String,
    pub(super) protected_tap_tip: String,
    merge_group: Option<MergeGroupSelection>,
}

pub(super) fn authenticate_tap_context(
    expected_tap_repository: &str,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &str,
    deadline: CheckDeadline,
) -> Result<TapContext, OrchestratorError> {
    let tap_repository_id = crate::package_release_publish::read_public_repository_identity(
        expected_tap_repository,
        deadline,
    )?;
    let (tap_base_sha, authenticated_head, protected_tap_tip, merge_group) = match event {
        WorkflowEvent::PullRequest => {
            let number = pull_request
                .filter(|number| *number > 0)
                .ok_or_else(|| internal("homebrew_authority_pull_request_missing"))?;
            let pull = crate::package_release_publish::read_public_pull_request(
                expected_tap_repository,
                tap_repository_id,
                number,
                deadline,
            )?;
            let (base_sha, head_sha) = admit_pull_request_context(
                &pull,
                expected_tap_repository,
                tap_repository_id,
                expected_head,
            )?;
            crate::package_release_publish::verify_public_pull_request_lineage(
                expected_tap_repository,
                tap_repository_id,
                "main",
                &base_sha,
                &head_sha,
                deadline,
            )?;
            (base_sha.clone(), head_sha, base_sha, None)
        }
        WorkflowEvent::Push => {
            if pull_request.is_some() {
                return Err(internal("homebrew_authority_unexpected_pull_request"));
            }
            let tip = crate::package_release_publish::verify_public_protected_main_lineage(
                expected_tap_repository,
                tap_repository_id,
                "main",
                expected_head,
                expected_head,
                deadline,
            )?;
            if tip != expected_head {
                return Err(internal("homebrew_authority_push_context"));
            }
            (tip.clone(), tip.clone(), tip, None)
        }
        WorkflowEvent::MergeGroup => {
            let selection = merge_group::authenticate(
                expected_tap_repository,
                tap_repository_id,
                expected_head,
                pull_request,
                deadline,
            )?;
            (
                selection.base_sha().to_owned(),
                selection.head_sha().to_owned(),
                selection.base_sha().to_owned(),
                Some(selection),
            )
        }
        _ => return Err(internal("homebrew_authority_event_unsupported")),
    };

    Ok(TapContext {
        tap_repository_id,
        tap_base_sha,
        authenticated_head,
        protected_tap_tip,
        merge_group,
    })
}

pub(super) fn recheck_tap_context(
    expected_tap_repository: &str,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &str,
    context: &TapContext,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let TapContext {
        tap_repository_id,
        tap_base_sha,
        authenticated_head,
        protected_tap_tip,
        merge_group,
    } = context;
    let tap_repository_id = *tap_repository_id;
    let final_tap_tip = match event {
        WorkflowEvent::PullRequest => {
            let number = pull_request
                .filter(|number| *number > 0)
                .ok_or_else(|| internal("homebrew_authority_pull_request_missing"))?;
            let pull = crate::package_release_publish::read_public_pull_request(
                expected_tap_repository,
                tap_repository_id,
                number,
                deadline,
            )?;
            let (observed_base, observed_head) = admit_pull_request_context(
                &pull,
                expected_tap_repository,
                tap_repository_id,
                expected_head,
            )?;
            if observed_base != *tap_base_sha || observed_head != *authenticated_head {
                return Err(internal("homebrew_authority_pull_request_changed"));
            }
            crate::package_release_publish::verify_public_pull_request_lineage(
                expected_tap_repository,
                tap_repository_id,
                "main",
                tap_base_sha,
                authenticated_head,
                deadline,
            )?;
            tap_base_sha.clone()
        }
        WorkflowEvent::Push => {
            crate::package_release_publish::verify_public_protected_main_lineage(
                expected_tap_repository,
                tap_repository_id,
                "main",
                expected_head,
                expected_head,
                deadline,
            )?
        }
        WorkflowEvent::MergeGroup => {
            let selection = merge_group
                .as_ref()
                .ok_or_else(|| internal("homebrew_authority_merge_group_missing"))?;
            crate::package_release_publish::verify_public_merge_group_context(selection, deadline)?;
            selection.base_sha().to_owned()
        }
        _ => return Err(internal("homebrew_authority_event_unsupported")),
    };
    if final_tap_tip != *protected_tap_tip {
        return Err(internal("homebrew_authority_tap_tip_changed"));
    }

    Ok(())
}
