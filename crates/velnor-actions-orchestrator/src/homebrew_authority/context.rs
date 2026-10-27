// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::{CheckDeadline, OrchestratorError, WorkflowEvent, internal};
use crate::package_release_publish::MergeGroupSelection;

#[path = "merge_group.rs"]
mod merge_group;
#[cfg(test)]
pub(crate) use merge_group::{RunnerContext, parse as parse_merge_group_event};
#[path = "pull_request_context.rs"]
pub(in crate::homebrew::authority) mod pull_request_context;

#[path = "qualification_context.rs"]
mod qualification_context;

pub(super) struct TapContext {
    pub(super) tap_repository_id: u64,
    pub(super) tap_base_sha: String,
    pub(super) authenticated_head: String,
    pub(super) checkout_head: String,
    pub(super) protected_tap_tip: String,
    merge_group: Option<MergeGroupSelection>,
    pull_request: Option<pull_request_context::PullRequestSelection>,
    qualification: Option<qualification_context::QualificationSelection>,
}

impl TapContext {
    pub(super) fn merge_group(&self) -> Option<&MergeGroupSelection> {
        self.merge_group.as_ref()
    }
}

struct EventAdmission {
    base_sha: String,
    authenticated_head: String,
    protected_tap_tip: String,
    merge_group: Option<MergeGroupSelection>,
    pull_request: Option<pull_request_context::PullRequestSelection>,
    qualification: Option<qualification_context::QualificationSelection>,
}

pub(super) fn authenticate_tap_context(
    root: &Path,
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
    let checkout_head = read_git_value(root, "HEAD", deadline)?;
    let admission = authenticate_event(
        &AdmissionRequest {
            root,
            repository: expected_tap_repository,
            repository_id: tap_repository_id,
            event,
            pull_request,
            expected_head,
            checkout_head: &checkout_head,
        },
        deadline,
    )?;
    Ok(TapContext {
        tap_repository_id,
        tap_base_sha: admission.base_sha,
        authenticated_head: admission.authenticated_head,
        checkout_head,
        protected_tap_tip: admission.protected_tap_tip,
        merge_group: admission.merge_group,
        pull_request: admission.pull_request,
        qualification: admission.qualification,
    })
}

pub(crate) fn verify_merge_group_checkout(
    root: &Path,
    selection: &MergeGroupSelection,
    repository: &str,
    repository_id: u64,
    expected_base: &str,
    expected_head: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if selection.repository() != repository
        || selection.repository_id() != repository_id
        || selection.base_sha() != expected_base
        || selection.head_sha() != expected_head
        || read_git_value(root, "HEAD", deadline)? != selection.head_sha()
    {
        return Err(internal("homebrew_authority_merge_group_checkout"));
    }
    Ok(())
}

pub(crate) fn read_git_value(
    root: &Path,
    revision: &str,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let result =
        velnor_actions_mise::GitRequest::rev_parse(vec![std::ffi::OsString::from(revision)])
            .run_in_until(root, deadline)
            .map_err(|_| internal("homebrew_authority_git_context"))?;
    result
        .require_success("git")
        .map_err(|_| internal("homebrew_authority_git_context"))?;
    let value = result
        .stdout_text("git")
        .map_err(|_| internal("homebrew_authority_git_context"))?;
    let value = value.trim();
    if !super::lowercase_sha(value) {
        return Err(internal("homebrew_authority_git_context"));
    }
    Ok(value.to_owned())
}

#[derive(Clone, Copy)]
struct AdmissionRequest<'a> {
    root: &'a Path,
    repository: &'a str,
    repository_id: u64,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &'a str,
    checkout_head: &'a str,
}

fn authenticate_event(
    request: &AdmissionRequest<'_>,
    deadline: CheckDeadline,
) -> Result<EventAdmission, OrchestratorError> {
    match request.event {
        WorkflowEvent::PullRequest | WorkflowEvent::Fork => {
            authenticate_pull_request(request, deadline)
        }
        WorkflowEvent::Push => authenticate_push(request, deadline),
        WorkflowEvent::MergeGroup => authenticate_merge_group(request, deadline),
        WorkflowEvent::Qualification => authenticate_qualification(request, deadline),
        WorkflowEvent::Local => Err(internal("homebrew_authority_event_unsupported")),
    }
}

fn authenticate_pull_request(
    request: &AdmissionRequest<'_>,
    deadline: CheckDeadline,
) -> Result<EventAdmission, OrchestratorError> {
    let AdmissionRequest {
        root,
        repository,
        repository_id,
        event,
        pull_request,
        expected_head,
        checkout_head,
    } = *request;
    let selection = pull_request_context::authenticate(
        &pull_request_context::AdmissionRequest {
            root,
            repository,
            repository_id,
            event,
            number: pull_request,
            expected_head,
            checkout_head,
        },
        deadline,
    )?;
    let base = selection.base_sha().to_owned();
    Ok(EventAdmission {
        base_sha: base.clone(),
        authenticated_head: selection.checkout_head().to_owned(),
        protected_tap_tip: base,
        merge_group: None,
        pull_request: Some(selection),
        qualification: None,
    })
}

fn authenticate_push(
    request: &AdmissionRequest<'_>,
    deadline: CheckDeadline,
) -> Result<EventAdmission, OrchestratorError> {
    reject_pull_number(request.pull_request)?;
    let tip = crate::package_release_publish::verify_public_protected_main_lineage(
        request.repository,
        request.repository_id,
        "main",
        request.expected_head,
        request.expected_head,
        deadline,
    )?;
    if !push_context_matches(&tip, request.expected_head, request.checkout_head) {
        return Err(internal("homebrew_authority_push_context"));
    }
    Ok(EventAdmission {
        base_sha: tip.clone(),
        authenticated_head: tip.clone(),
        protected_tap_tip: tip,
        merge_group: None,
        pull_request: None,
        qualification: None,
    })
}

fn authenticate_merge_group(
    request: &AdmissionRequest<'_>,
    deadline: CheckDeadline,
) -> Result<EventAdmission, OrchestratorError> {
    let selection = merge_group::authenticate(
        request.repository,
        request.repository_id,
        request.expected_head,
        request.pull_request,
        deadline,
    )?;
    if request.checkout_head != selection.head_sha() {
        return Err(internal("homebrew_authority_merge_group_checkout"));
    }
    Ok(EventAdmission {
        base_sha: selection.base_sha().to_owned(),
        authenticated_head: selection.head_sha().to_owned(),
        protected_tap_tip: selection.base_sha().to_owned(),
        merge_group: Some(selection),
        pull_request: None,
        qualification: None,
    })
}

fn authenticate_qualification(
    request: &AdmissionRequest<'_>,
    deadline: CheckDeadline,
) -> Result<EventAdmission, OrchestratorError> {
    reject_pull_number(request.pull_request)?;
    let selection = qualification_context::authenticate(
        request.repository,
        request.repository_id,
        request.expected_head,
        request.checkout_head,
        deadline,
    )?;
    let source = selection.source_sha().to_owned();
    Ok(EventAdmission {
        base_sha: source.clone(),
        authenticated_head: source.clone(),
        protected_tap_tip: source,
        merge_group: None,
        pull_request: None,
        qualification: Some(selection),
    })
}

pub(super) fn recheck_tap_context(
    root: &Path,
    expected_tap_repository: &str,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &str,
    context: &TapContext,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let final_tap_tip = recheck_event_context(
        root,
        expected_tap_repository,
        event,
        pull_request,
        expected_head,
        context,
        deadline,
    )?;
    if final_tap_tip != context.protected_tap_tip
        || read_git_value(root, "HEAD", deadline)? != context.checkout_head
    {
        return Err(internal("homebrew_authority_tap_context_changed"));
    }
    Ok(())
}

fn recheck_event_context(
    root: &Path,
    expected_tap_repository: &str,
    event: WorkflowEvent,
    pull_request: Option<u64>,
    expected_head: &str,
    context: &TapContext,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let TapContext {
        tap_repository_id,
        tap_base_sha,
        authenticated_head,
        checkout_head,
        merge_group,
        pull_request: pr,
        qualification,
        ..
    } = context;
    let final_tap_tip = match event {
        WorkflowEvent::PullRequest | WorkflowEvent::Fork => {
            let selection = pr
                .as_ref()
                .ok_or_else(|| internal("homebrew_authority_pull_request_missing"))?;
            pull_request_context::recheck(
                &pull_request_context::RecheckRequest {
                    root,
                    repository: expected_tap_repository,
                    repository_id: *tap_repository_id,
                    event,
                    number: pull_request,
                    expected_head,
                    selection,
                },
                deadline,
            )?;
            tap_base_sha.clone()
        }
        WorkflowEvent::Push => {
            reject_pull_number(pull_request)?;
            let tip = crate::package_release_publish::verify_public_protected_main_lineage(
                expected_tap_repository,
                *tap_repository_id,
                "main",
                expected_head,
                expected_head,
                deadline,
            )?;
            if !push_context_matches(&tip, authenticated_head, checkout_head) {
                return Err(internal("homebrew_authority_push_context_changed"));
            }
            tip
        }
        WorkflowEvent::MergeGroup => {
            reject_pull_number(pull_request)?;
            let selection = merge_group
                .as_ref()
                .ok_or_else(|| internal("homebrew_authority_merge_group_missing"))?;
            crate::package_release_publish::verify_public_merge_group_context(selection, deadline)?;
            if selection.head_sha() != checkout_head {
                return Err(internal("homebrew_authority_merge_group_checkout"));
            }
            selection.base_sha().to_owned()
        }
        WorkflowEvent::Qualification => {
            reject_pull_number(pull_request)?;
            let selection = qualification
                .as_ref()
                .ok_or_else(|| internal("homebrew_authority_qualification_missing"))?;
            qualification_context::recheck(
                expected_tap_repository,
                *tap_repository_id,
                expected_head,
                selection,
                deadline,
            )?;
            tap_base_sha.clone()
        }
        WorkflowEvent::Local => return Err(internal("homebrew_authority_event_unsupported")),
    };
    Ok(final_tap_tip)
}

fn reject_pull_number(number: Option<u64>) -> Result<(), OrchestratorError> {
    if number.is_some() {
        return Err(internal("homebrew_authority_unexpected_pull_request"));
    }
    Ok(())
}

fn push_context_matches(tip: &str, expected_head: &str, checkout_head: &str) -> bool {
    tip == expected_head && checkout_head == tip
}
