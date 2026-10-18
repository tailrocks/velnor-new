//! GitHub Actions workflow execution protection policy (2026-09-17).
//!
//! Models the repository and organization execution protections from
//! `/repos/{owner}/{repo}/actions/policies` (`restrict_actions_actors` and
//! `restrict_action_events`) and the November 2, 2026 default block of
//! `pull_request_target` on public repositories. Velnor workflows admit only
//! the five audited trigger events in [`ALLOWED_WORKFLOW_EVENTS`] and fail
//! closed on every untrusted-actor event in [`RESTRICTED_ACTION_EVENTS`].

use crate::errors::ContractError;

/// Exact allowlist of workflow trigger events Velnor workflows may emit.
pub const ALLOWED_WORKFLOW_EVENTS: [&str; 5] = [
    "pull_request",
    "push",
    "merge_group",
    "schedule",
    "workflow_dispatch",
];

/// Untrusted-actor trigger events restricted by GitHub's `restrict_action_events`
/// policy (`2026-09-17`) and forbidden in all Velnor workflows.
pub const RESTRICTED_ACTION_EVENTS: [&str; 11] = [
    "pull_request_target",
    "workflow_run",
    "issue_comment",
    "issues",
    "discussion",
    "discussion_comment",
    "fork",
    "gollum",
    "public",
    "pull_request_review",
    "pull_request_review_comment",
];

/// Trigger events blocked by default on public repositories starting 2026-11-02.
pub const DEFAULT_BLOCKED_PUBLIC_REPO_EVENTS: [&str; 1] = ["pull_request_target"];

/// True when `event` is one of Velnor's audited workflow trigger events.
#[must_use]
pub fn is_allowed_workflow_event(event: &str) -> bool {
    ALLOWED_WORKFLOW_EVENTS.contains(&event)
}

/// True when `event` is an untrusted-actor trigger covered by GitHub's
/// `restrict_action_events` protection (`2026-09-17`).
#[must_use]
pub fn is_restricted_action_event(event: &str) -> bool {
    RESTRICTED_ACTION_EVENTS.contains(&event)
}

/// Validate a single workflow trigger event name against Velnor's execution
/// protection policy.
///
/// # Errors
///
/// Returns [`ContractError::InvalidIdentity`] with
/// `restricted_untrusted_actor_event:<event>` for any event in
/// [`RESTRICTED_ACTION_EVENTS`], or `unsupported_trigger_event:<event>` for
/// any other event outside [`ALLOWED_WORKFLOW_EVENTS`].
pub fn validate_workflow_trigger_event(event: &str) -> Result<(), ContractError> {
    if is_restricted_action_event(event) {
        return Err(ContractError::identity(
            "trigger.event",
            format!("restricted_untrusted_actor_event:{event}"),
        ));
    }
    if !is_allowed_workflow_event(event) {
        return Err(ContractError::identity(
            "trigger.event",
            format!("unsupported_trigger_event:{event}"),
        ));
    }
    Ok(())
}

/// Validate a non-empty sequence of workflow trigger event names.
///
/// # Errors
///
/// Returns [`ContractError::InvalidIdentity`] when the sequence is empty or
/// contains any restricted or unsupported trigger event.
pub fn validate_workflow_trigger_events<'a>(
    events: impl IntoIterator<Item = &'a str>,
) -> Result<(), ContractError> {
    let mut seen_any = false;
    for event in events {
        seen_any = true;
        validate_workflow_trigger_event(event)?;
    }
    if !seen_any {
        return Err(ContractError::identity("trigger.event", "empty_events"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{
        CACHE_MODE_PUSH_WRITE_EXPR, CacheMode, Concurrency, Job, JobTimeout, Permissions, Step,
        StepKind, Trigger, WorkflowIr,
    };
    use std::collections::BTreeMap;

    #[test]
    fn restricted_action_events_cover_all_eleven_github_execution_protection_triggers() {
        let expected = [
            "pull_request_target",
            "workflow_run",
            "issue_comment",
            "issues",
            "discussion",
            "discussion_comment",
            "fork",
            "gollum",
            "public",
            "pull_request_review",
            "pull_request_review_comment",
        ];
        assert_eq!(RESTRICTED_ACTION_EVENTS, expected);
        assert_eq!(DEFAULT_BLOCKED_PUBLIC_REPO_EVENTS, ["pull_request_target"]);
        for event in RESTRICTED_ACTION_EVENTS {
            assert!(is_restricted_action_event(event), "{event}");
            assert!(!is_allowed_workflow_event(event), "{event}");
            let err = validate_workflow_trigger_event(event).expect_err("must reject");
            assert_eq!(
                err,
                ContractError::identity(
                    "trigger.event",
                    format!("restricted_untrusted_actor_event:{event}")
                )
            );
            assert!(
                validate_workflow_trigger_events(["pull_request", event]).is_err(),
                "{event}"
            );
        }
    }

    #[test]
    fn allowed_workflow_events_accept_only_audited_five() {
        for event in ALLOWED_WORKFLOW_EVENTS {
            assert!(is_allowed_workflow_event(event), "{event}");
            assert!(!is_restricted_action_event(event), "{event}");
            assert_eq!(validate_workflow_trigger_event(event), Ok(()));
        }
        assert_eq!(
            validate_workflow_trigger_events(ALLOWED_WORKFLOW_EVENTS),
            Ok(())
        );
        assert_eq!(
            validate_workflow_trigger_events(std::iter::empty::<&str>()),
            Err(ContractError::identity("trigger.event", "empty_events"))
        );
        assert_eq!(
            validate_workflow_trigger_event("repository_dispatch"),
            Err(ContractError::identity(
                "trigger.event",
                "unsupported_trigger_event:repository_dispatch"
            ))
        );
    }

    #[test]
    fn cache_mode_parses_and_validates_workflow_and_job_levels() {
        for (raw, mode) in [
            ("read", CacheMode::Read),
            ("write", CacheMode::Write),
            ("write-only", CacheMode::WriteOnly),
            ("none", CacheMode::None),
            (
                CACHE_MODE_PUSH_WRITE_EXPR,
                CacheMode::push_write_expression(),
            ),
        ] {
            assert_eq!(CacheMode::parse(raw), Ok(mode.clone()));
            assert_eq!(mode.as_str(), raw);
            assert_eq!(mode.validate("workflow.cache_mode"), Ok(()));
        }
        assert!(CacheMode::parse("read-write").is_err());
        assert!(
            CacheMode::Expression("not-an-expr".to_owned())
                .validate("workflow.cache_mode")
                .is_err()
        );
        assert!(
            CacheMode::Expression("${{ github.event_name == 'push'\n}}".to_owned())
                .validate("workflow.cache_mode")
                .is_err()
        );

        let mut ir = WorkflowIr {
            name: "CI".to_owned(),
            triggers: Trigger {
                pull_request_types: vec!["opened".to_owned()],
                push_branches: vec!["main".to_owned()],
                merge_group: true,
                workflow_dispatch: None,
                workflow_call: false,
                schedule: None,
            },
            permissions: Permissions::default(),
            cache_mode: Some(CacheMode::push_write_expression()),
            concurrency: Concurrency {
                group: "ci".to_owned(),
                cancel_in_progress: "false".to_owned(),
            },
            jobs: BTreeMap::from([(
                "plan".to_owned(),
                Job {
                    display_name: "Plan".to_owned(),
                    runs_on: "ubuntu-26.04".to_owned(),
                    check_runner: None,
                    timeout_minutes: JobTimeout::PLAN,
                    needs: Vec::new(),
                    condition: None,
                    permissions: None,
                    cache_mode: Some(CacheMode::Read),
                    environment: None,
                    steps: vec![Step {
                        name: "Plan".to_owned(),
                        id: None,
                        role: None,
                        condition: None,
                        kind: StepKind::Internal {
                            operation: "plan-v1".to_owned(),
                            env: BTreeMap::new(),
                        },
                    }],
                },
            )]),
        };
        assert_eq!(ir.validate(), Ok(()));
        ir.triggers.pull_request_types = vec!["pull_request_target".to_owned()];
        assert!(ir.validate().is_err());
    }
}
