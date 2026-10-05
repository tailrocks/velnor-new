use super::*;

fn facts(event: WorkflowEvent, git_ref: &str) -> CacheWriterFacts {
    CacheWriterFacts {
        event: Some(event),
        git_ref: Some(git_ref.to_owned()),
        default_branch: Some("release/2026".to_owned()),
        repository: Some("O/R".to_owned()),
        event_repository: Some("o/r".to_owned()),
    }
}

fn observed(
    facts: &CacheWriterFacts,
    current_default: Option<&str>,
    protected: Option<bool>,
) -> CacheWriterContext {
    CacheWriterContext::from_observation(facts, current_default, protected)
}

#[test]
fn api_target_is_current_repository_default_branch_only() {
    assert_eq!(
        api_query_inputs(&facts(WorkflowEvent::Push, "refs/heads/release/2026")),
        Some((
            "o/r".to_owned(),
            "release/2026".to_owned(),
            "release%2F2026".to_owned(),
        ))
    );
    for denied in [
        facts(WorkflowEvent::PullRequest, "refs/heads/release/2026"),
        facts(WorkflowEvent::Push, "refs/heads/feature"),
        facts(WorkflowEvent::Push, "refs/tags/v1"),
    ] {
        assert_eq!(api_query_inputs(&denied), None);
    }
    assert_eq!(
        repository_default_args("o/r").map(|arg| arg.to_string_lossy().into_owned()),
        [
            "api",
            "repos/o/r",
            "--hostname",
            "github.com",
            "--jq",
            ".default_branch"
        ]
    );
    assert_eq!(
        branch_protection_args("o/r", "release%2F2026")
            .map(|arg| arg.to_string_lossy().into_owned()),
        [
            "api",
            "repos/o/r/branches/release%2F2026",
            "--hostname",
            "github.com",
            "--jq",
            ".protected"
        ]
    );
}

#[test]
fn api_target_rejects_unanchored_repository_and_hostile_branch() {
    let mut unanchored = facts(WorkflowEvent::Push, "refs/heads/release/2026");
    unanchored.event_repository = Some("attacker/other".to_owned());
    assert_eq!(api_query_inputs(&unanchored), None);

    let mut hostile = facts(WorkflowEvent::Push, "refs/heads/release/2026");
    hostile.default_branch = Some("release/?token".to_owned());
    hostile.git_ref = Some("refs/heads/release/?token".to_owned());
    assert_eq!(api_query_inputs(&hostile), None);

    let mut hostile_repo = facts(WorkflowEvent::Push, "refs/heads/release/2026");
    hostile_repo.repository = Some("o?token/r".to_owned());
    hostile_repo.event_repository = Some("o?token/r".to_owned());
    assert_eq!(api_query_inputs(&hostile_repo), None);
}

#[test]
fn request_facts_must_match_runner_event_and_repository_sources() {
    let runner = facts(WorkflowEvent::Push, "refs/heads/release/2026");
    assert!(request_matches_runner_facts(&runner, &runner));
    let mut mismatched_event = runner.clone();
    mismatched_event.event = Some(WorkflowEvent::PullRequest);
    let mut mismatched_ref = runner.clone();
    mismatched_ref.git_ref = Some("refs/heads/feature".to_owned());
    let mut mismatched_default = runner.clone();
    mismatched_default.default_branch = Some("main".to_owned());
    let mut mismatched_run_repo = runner.clone();
    mismatched_run_repo.repository = Some("other/r".to_owned());
    let mut mismatched_event_repo = runner.clone();
    mismatched_event_repo.event_repository = Some("other/r".to_owned());
    for request in [
        mismatched_event,
        mismatched_ref,
        mismatched_default,
        mismatched_run_repo,
        mismatched_event_repo,
    ] {
        assert!(!request_matches_runner_facts(&request, &runner));
    }
}

#[test]
fn inconsistent_public_query_inputs_fall_back_to_cold_context() {
    let facts = facts(WorkflowEvent::PullRequest, "refs/heads/release/2026");
    let context = context_for_facts(
        &facts,
        WorkflowEvent::Push,
        &ToolCatalog::pinned(),
        Path::new("."),
    );
    assert_eq!(context.trust(), Trust::Pr);
    assert!(!context.permits_trusted_write());
    assert!(!CacheWriterContext::default().permits_trusted_write());
}

#[test]
fn only_complete_matching_observations_authorize_trusted_writes() {
    let valid = facts(WorkflowEvent::Push, "refs/heads/release/2026");
    assert!(observed(&valid, Some("release/2026"), Some(true)).permits_trusted_write());

    let mut mismatched_repo = valid.clone();
    mismatched_repo.event_repository = Some("fork/r".to_owned());
    let mut feature_ref = valid.clone();
    feature_ref.git_ref = Some("refs/heads/feature".to_owned());
    let mut pull_request = valid.clone();
    pull_request.event = Some(WorkflowEvent::PullRequest);
    let mut mismatched_default = valid.clone();
    mismatched_default.default_branch = Some("old".to_owned());
    for (facts, current, protected) in [
        (&valid, None, None),
        (&valid, Some("main"), Some(true)),
        (&valid, Some("release/2026"), Some(false)),
        (&valid, Some("release/2026"), None),
        (&mismatched_repo, Some("release/2026"), Some(true)),
        (&feature_ref, Some("release/2026"), Some(true)),
        (&mismatched_default, Some("release/2026"), Some(true)),
        (&pull_request, Some("release/2026"), Some(true)),
    ] {
        assert!(
            !observed(facts, current, protected).permits_trusted_write(),
            "incomplete or mismatched facts cannot authorize"
        );
    }
    assert!(!CacheWriterContext::default().permits_trusted_write());
}

#[test]
fn save_gates_need_opaque_verified_context_and_all_runtime_conditions() {
    use crate::cache_trust::{SaveGate, save_after_success};
    use crate::restore::{MissReason, SaveInputs, save_decision};

    let valid = facts(WorkflowEvent::Push, "refs/heads/release/2026");
    let trusted = observed(&valid, Some("release/2026"), Some(true));
    let cold = CacheWriterContext::default();
    assert!(crate::cache::save_allowed("trusted", &trusted, true));
    assert!(!crate::cache::save_allowed("trusted", &trusted, false));
    assert!(!crate::cache::save_allowed("pr", &trusted, true));
    assert!(!crate::cache::save_allowed("trusted", &cold, true));

    let open = SaveGate {
        producer_passed: true,
        useful_delta: true,
        writer: &trusted,
        writers_finished: true,
    };
    assert!(save_after_success(open));
    for gate in [
        SaveGate {
            producer_passed: false,
            ..open
        },
        SaveGate {
            useful_delta: false,
            ..open
        },
        SaveGate {
            writer: &cold,
            ..open
        },
        SaveGate {
            writers_finished: false,
            ..open
        },
    ] {
        assert!(!save_after_success(gate));
    }

    let allowed = SaveInputs {
        layer_trust: "trusted",
        writer: &trusted,
        passed: true,
        unavailable: false,
        active_writer: false,
    };
    assert_eq!(save_decision(&allowed), Ok(()));
    for denied in [
        SaveInputs {
            passed: false,
            ..allowed
        },
        SaveInputs {
            unavailable: true,
            ..allowed
        },
        SaveInputs {
            active_writer: true,
            ..allowed
        },
        SaveInputs {
            layer_trust: "pr",
            ..allowed
        },
        SaveInputs {
            writer: &cold,
            ..allowed
        },
    ] {
        let expected = if denied.unavailable {
            MissReason::CACHE_UNAVAILABLE
        } else {
            MissReason::CACHE_WRITE_DISABLED
        };
        assert_eq!(save_decision(&denied), Err(expected));
    }
}

#[test]
fn official_api_boolean_parser_is_strict() {
    assert_eq!(parse_bool("true\n"), Some(true));
    assert_eq!(parse_bool("false\n"), Some(false));
    for value in ["", "TRUE", "null", "true false", "1"] {
        assert_eq!(parse_bool(value), None);
    }
}

#[test]
fn api_path_segment_encodes_slashes_and_non_ascii() {
    assert_eq!(encode_segment("release/2026"), "release%2F2026");
    assert_eq!(encode_segment("main"), "main");
    assert_eq!(encode_segment("café"), "caf%C3%A9");
    assert_eq!(encode_segment("branch$token"), "branch%24token");
}
