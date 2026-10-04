//! Cache trust derives from live runner facts at each operation boundary.

use std::path::Path;

use velnor_actions_contract::{Trust, WorkflowEvent, parse_strict_json, trust_for_event};

use super::AnalysisPublicationContext;
use crate::prepare::GenerationPreparation;

impl AnalysisPublicationContext {
    /// Neither request JSON nor downloaded plan artifacts can confer trust.
    pub(crate) fn runner_trust(
        event: WorkflowEvent,
        head: &str,
        prep: Option<&GenerationPreparation>,
    ) -> Trust {
        let context = runner_context();
        let context = context
            .as_ref()
            .filter(|context| prep.is_none_or(|prep| context.qualifies(prep, head)));
        qualified_trust(event, head, context)
    }
}

/// Serialized publication facts must exactly match a fresh runner capture.
/// The request supplies expectations; only this operation boundary supplies authority.
pub(super) fn publication_context_matches(context: &AnalysisPublicationContext) -> bool {
    bound_publication_context(context, runner_context().as_ref())
}

fn bound_publication_context(
    requested: &AnalysisPublicationContext,
    actual: Option<&AnalysisPublicationContext>,
) -> bool {
    actual == Some(requested)
}

fn qualified_trust(
    event: WorkflowEvent,
    head: &str,
    context: Option<&AnalysisPublicationContext>,
) -> Trust {
    if event == WorkflowEvent::Push
        && context.is_some_and(|context| context.matches_source(head, &context.default_branch))
    {
        Trust::Trusted
    } else {
        trust_for_event(event)
    }
}

fn runner_context() -> Option<AnalysisPublicationContext> {
    let path = std::env::var_os("GITHUB_EVENT_PATH").filter(|value| !value.is_empty())?;
    let text =
        crate::safe_read::read_event_file(Path::new(&path), crate::safe_read::MAX_REPO_FILE_BYTES)
            .ok()?;
    let payload = parse_strict_json(&text).ok()?;
    let context = AnalysisPublicationContext::capture(&payload)?;
    let actual_ref = std::env::var("GITHUB_REF").ok()?;
    if actual_ref != format!("refs/heads/{}", context.default_branch) {
        return None;
    }
    Some(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protected_context() -> AnalysisPublicationContext {
        let head = "a".repeat(40);
        AnalysisPublicationContext {
            repository: "owner/repo".to_owned(),
            head: head.clone(),
            workflow_sha: head,
            workflow_ref: format!(
                "owner/repo/{}@refs/heads/main",
                velnor_actions_workflow_renderer::WORKFLOW_PATH
            ),
            branch: "main".to_owned(),
            default_branch: "main".to_owned(),
            run_id: 1,
            run_attempt: 1,
            protected: true,
            event: "push".to_owned(),
        }
    }

    fn assert_read_only(event: WorkflowEvent, context: Option<&AnalysisPublicationContext>) {
        assert_eq!(qualified_trust(event, &"a".repeat(40), context), Trust::Pr);
    }

    #[test]
    fn publication_json_cannot_replace_live_protection_or_attempt_identity() {
        let requested = protected_context();
        assert!(bound_publication_context(&requested, Some(&requested)));
        assert!(!bound_publication_context(&requested, None));
        for field in [
            "protected",
            "run_id",
            "run_attempt",
            "workflow_sha",
            "repository",
        ] {
            let mut actual = requested.clone();
            match field {
                "protected" => actual.protected = false,
                "run_id" => actual.run_id += 1,
                "run_attempt" => actual.run_attempt += 1,
                "workflow_sha" => actual.workflow_sha = "b".repeat(40),
                "repository" => actual.repository = "fork/repo".to_owned(),
                _ => unreachable!(),
            }
            assert!(
                !bound_publication_context(&requested, Some(&actual)),
                "{field}"
            );
        }
    }

    #[test]
    fn protection_is_required_for_trusted_push() {
        let context = protected_context();
        assert_eq!(
            qualified_trust(WorkflowEvent::Push, &context.head, Some(&context)),
            Trust::Trusted
        );
        assert_read_only(WorkflowEvent::Push, None);
        for event in [
            WorkflowEvent::PullRequest,
            WorkflowEvent::MergeGroup,
            WorkflowEvent::WorkflowDispatch,
        ] {
            assert_read_only(event, Some(&context));
        }
        let mut unprotected = context;
        unprotected.protected = false;
        assert_read_only(WorkflowEvent::Push, Some(&unprotected));
    }

    #[test]
    fn source_authority_is_required_for_trusted_push() {
        for field in [
            "branch",
            "default_branch",
            "workflow_sha",
            "workflow_ref",
            "head",
            "event",
            "repository",
        ] {
            let mut invalid = protected_context();
            match field {
                "branch" => invalid.branch = "topic".to_owned(),
                "default_branch" => invalid.default_branch = "topic".to_owned(),
                "workflow_sha" => invalid.workflow_sha = "b".repeat(40),
                "workflow_ref" => invalid.workflow_ref.push_str("-forged"),
                "head" => invalid.head = "b".repeat(40),
                "event" => invalid.event = "workflow_dispatch".to_owned(),
                "repository" => invalid.repository = "fork/repo".to_owned(),
                _ => unreachable!(),
            }
            assert_read_only(WorkflowEvent::Push, Some(&invalid));
        }
    }
}
