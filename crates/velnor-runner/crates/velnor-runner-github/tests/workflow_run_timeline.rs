//! Full pull request base history checks.

use std::collections::VecDeque;

use velnor_runner_github::workflow_run::{
    BaseRefChangedEvent, PullRequestBaseHistory, RepositoryTarget, WorkflowRunError,
    get_pull_request_base_history, new_retarget_event, ordered_base_changes,
    retarget_event_after_acknowledgement,
};
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

const OWNER: &str = "ChainArgos";
const REPOSITORY: &str = "java-monorepo";
const REPOSITORY_ID: i64 = 829_618_808;
const HEAD_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn new(bodies: &[String]) -> Self {
        Self {
            replies: bodies
                .iter()
                .map(|body| {
                    Ok(Exchange {
                        status: 200,
                        body: body.as_bytes().to_vec(),
                    })
                })
                .collect(),
            seen: Vec::new(),
        }
    }

    fn failure(failure: TransportFail) -> Self {
        Self {
            replies: VecDeque::from([Err(failure)]),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.replies
            .pop_front()
            .unwrap_or(Err(TransportFail::Reset))
    }
}

fn expected() -> RepositoryTarget<'static> {
    RepositoryTarget {
        id: REPOSITORY_ID,
        owner: OWNER,
        name: REPOSITORY,
    }
}

fn page(
    base_ref: &str,
    base_sha: &str,
    events: &[(&str, &str, &str)],
    has_next: bool,
    cursor: Option<&str>,
    updated_at: &str,
) -> String {
    let nodes = events
        .iter()
        .map(|(id, at, refs)| {
            let (previous, current) = refs.split_once('>').unwrap_or(("", ""));
            serde_json::json!({
                "__typename": "BaseRefChangedEvent",
                "id": id,
                "databaseId": 7,
                "createdAt": at,
                "previousRefName": previous,
                "currentRefName": current,
                "pullRequest": {
                    "number": 2110,
                    "repository": {
                        "databaseId": REPOSITORY_ID,
                        "name": REPOSITORY,
                        "owner": {"login": OWNER}
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "data": {
            "repository": {
                "databaseId": REPOSITORY_ID,
                "name": REPOSITORY,
                "owner": {"login": OWNER},
                "pullRequest": {
                    "number": 2110,
                    "updatedAt": updated_at,
                    "headRefName": "fix/runner",
                    "headRefOid": HEAD_SHA,
                    "headRepository": {
                        "databaseId": REPOSITORY_ID,
                        "name": REPOSITORY,
                        "owner": {"login": OWNER}
                    },
                    "baseRefName": base_ref,
                    "baseRefOid": base_sha,
                    "baseRepository": {
                        "databaseId": REPOSITORY_ID,
                        "name": REPOSITORY,
                        "owner": {"login": OWNER}
                    },
                    "timelineItems": {
                        "nodes": nodes,
                        "pageInfo": {
                            "hasNextPage": has_next,
                            "endCursor": cursor
                        }
                    }
                }
            }
        }
    })
    .to_string()
}

fn event(id: &str, at: &str, previous: &str, current: &str) -> BaseRefChangedEvent {
    BaseRefChangedEvent {
        id: id.to_owned(),
        database_id: Some(7),
        created_at: at.to_owned(),
        previous_ref_name: previous.to_owned(),
        current_ref_name: current.to_owned(),
    }
}

fn history(base_ref: &str, events: Vec<BaseRefChangedEvent>) -> PullRequestBaseHistory {
    PullRequestBaseHistory {
        number: 2110,
        repository_id: REPOSITORY_ID,
        head_repository_id: REPOSITORY_ID,
        head_ref: "fix/runner".to_owned(),
        head_sha: HEAD_SHA.to_owned(),
        base_repository_id: REPOSITORY_ID,
        base_ref: base_ref.to_owned(),
        base_sha: BASE_SHA.to_owned(),
        updated_at: "2026-10-18T00:00:00Z".to_owned(),
        events,
    }
}

#[test]
fn reads_all_pages_and_checks_exact_repository_and_pr() -> Result<(), String> {
    let first = page(
        "sea/integration",
        BASE_SHA,
        &[("node-7", "2026-10-17T00:00:00Z", "main>sea/integration")],
        true,
        Some("cursor-1"),
        "2026-10-18T00:00:00Z",
    );
    let second = page(
        "sea/integration",
        BASE_SHA,
        &[],
        false,
        None,
        "2026-10-18T00:00:00Z",
    );
    let mut script = Script::new(&[first, second]);
    let result = get_pull_request_base_history(&mut script, expected(), 2110, "canary-token")
        .map_err(|error| error.to_string())?;
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].id, "node-7");
    assert_eq!(script.seen.len(), 2);
    let request = script.seen.first().ok_or("first request")?;
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.path, "/graphql");
    assert!(
        request
            .headers
            .iter()
            .any(|(name, value)| name == "Authorization" && value == "Bearer canary-token")
    );
    let body = String::from_utf8(request.body.clone()).map_err(|_| "request body")?;
    assert!(body.contains("BASE_REF_CHANGED_EVENT"));
    assert!(!body.contains("canary-token"));
    let second_body = String::from_utf8(script.seen.get(1).ok_or("second request")?.body.clone())
        .map_err(|_| "second body")?;
    assert!(second_body.contains("cursor-1"));
    Ok(())
}

#[test]
fn rejects_repeated_cursor_and_changed_snapshot_between_pages() {
    let repeated = page(
        "sea/integration",
        BASE_SHA,
        &[],
        true,
        Some("cursor"),
        "2026-10-18T00:00:00Z",
    );
    let mut script = Script::new(&[repeated.clone(), repeated.clone()]);
    assert_eq!(
        get_pull_request_base_history(&mut script, expected(), 2110, "token"),
        Err(WorkflowRunError::Malformed)
    );

    let first = page(
        "sea/integration",
        BASE_SHA,
        &[],
        true,
        Some("next"),
        "2026-10-18T00:00:00Z",
    );
    let changed = page("main", HEAD_SHA, &[], false, None, "2026-10-18T00:00:01Z");
    let mut script = Script::new(&[first, changed]);
    assert_eq!(
        get_pull_request_base_history(&mut script, expected(), 2110, "token"),
        Err(WorkflowRunError::Malformed)
    );
}

#[test]
fn rejects_missing_cursor_duplicate_events_and_uncertain_transport() {
    let missing_cursor = page(
        "sea/integration",
        BASE_SHA,
        &[],
        true,
        None,
        "2026-10-18T00:00:00Z",
    );
    let mut script = Script::new(&[missing_cursor]);
    assert_eq!(
        get_pull_request_base_history(&mut script, expected(), 2110, "token"),
        Err(WorkflowRunError::Malformed)
    );

    let event = ("node-7", "2026-10-17T00:00:00Z", "main>sea/integration");
    let first = page(
        "sea/integration",
        BASE_SHA,
        &[event],
        true,
        Some("cursor-1"),
        "2026-10-18T00:00:00Z",
    );
    let second = page(
        "sea/integration",
        BASE_SHA,
        &[event],
        false,
        None,
        "2026-10-18T00:00:00Z",
    );
    let mut script = Script::new(&[first, second]);
    assert_eq!(
        get_pull_request_base_history(&mut script, expected(), 2110, "token"),
        Err(WorkflowRunError::Malformed)
    );

    let mut script = Script::failure(TransportFail::Timeout);
    assert_eq!(
        get_pull_request_base_history(&mut script, expected(), 2110, "token"),
        Err(WorkflowRunError::Uncertain)
    );
}

#[test]
fn rejects_missing_or_foreign_event_scope() -> Result<(), String> {
    let populated = page(
        "sea/integration",
        BASE_SHA,
        &[("node-7", "2026-10-17T00:00:00Z", "main>sea/integration")],
        false,
        None,
        "2026-10-18T00:00:00Z",
    );
    let mut event_value: serde_json::Value =
        serde_json::from_str(&populated).map_err(|_| "event fixture JSON")?;
    let node = event_value
        .pointer_mut("/data/repository/pullRequest/timelineItems/nodes/0/pullRequest")
        .ok_or("event pull request")?;
    node["repository"] = serde_json::Value::Null;
    let missing_repository = event_value.to_string();
    let mut script = Script::new(&[missing_repository]);
    assert_eq!(
        get_pull_request_base_history(&mut script, expected(), 2110, "token"),
        Err(WorkflowRunError::Malformed)
    );

    let wrong_root = page(
        "sea/integration",
        BASE_SHA,
        &[],
        false,
        None,
        "2026-10-18T00:00:00Z",
    )
    .replace(&REPOSITORY_ID.to_string(), "1");
    let mut script = Script::new(&[wrong_root]);
    assert_eq!(
        get_pull_request_base_history(&mut script, expected(), 2110, "token"),
        Err(WorkflowRunError::RepositoryMismatch)
    );
    Ok(())
}

#[test]
fn base_history_requires_unique_times_a_continuous_chain_and_current_base() -> Result<(), String> {
    let valid = history(
        "sea/integration",
        vec![
            event("first", "2026-10-17T00:00:00Z", "main", "staging"),
            event(
                "second",
                "2026-10-17T00:00:01Z",
                "staging",
                "sea/integration",
            ),
        ],
    );
    assert_eq!(
        ordered_base_changes(&valid)
            .map_err(|error| error.to_string())?
            .len(),
        2
    );
    assert_eq!(
        retarget_event_after_acknowledgement(&valid, "second", "staging", "sea/integration")
            .map_err(|error| error.to_string())?
            .id,
        "second"
    );
    let tie = history(
        "sea/integration",
        vec![
            event("first", "2026-10-17T00:00:00Z", "main", "staging"),
            event(
                "second",
                "2026-10-17T00:00:00Z",
                "staging",
                "sea/integration",
            ),
        ],
    );
    assert_eq!(ordered_base_changes(&tie), Err(WorkflowRunError::Malformed));
    let gap = history(
        "sea/integration",
        vec![event(
            "first",
            "2026-10-17T00:00:00Z",
            "main",
            "sea/integration",
        )],
    );
    let mut gap = gap;
    gap.events.push(event(
        "second",
        "2026-10-17T00:00:01Z",
        "other",
        "sea/integration",
    ));
    assert_eq!(ordered_base_changes(&gap), Err(WorkflowRunError::Malformed));
    let wrong_current = history(
        "main",
        vec![event(
            "first",
            "2026-10-17T00:00:00Z",
            "main",
            "sea/integration",
        )],
    );
    assert_eq!(
        ordered_base_changes(&wrong_current),
        Err(WorkflowRunError::Malformed)
    );
    Ok(())
}

#[test]
fn retarget_receipt_needs_exactly_one_appended_final_event() -> Result<(), String> {
    let before = history("main", Vec::new());
    let after = history(
        "sea/integration",
        vec![event(
            "acknowledged-event",
            "2026-10-17T00:00:00Z",
            "main",
            "sea/integration",
        )],
    );
    assert_eq!(
        new_retarget_event(&before, &after, "main", "sea/integration")
            .map_err(|error| error.to_string())?
            .id,
        "acknowledged-event"
    );
    let later = history(
        "main",
        vec![
            event(
                "acknowledged-event",
                "2026-10-17T00:00:00Z",
                "main",
                "sea/integration",
            ),
            event(
                "later-event",
                "2026-10-17T00:00:01Z",
                "sea/integration",
                "main",
            ),
        ],
    );
    assert_eq!(
        new_retarget_event(&before, &later, "main", "sea/integration"),
        Err(WorkflowRunError::Malformed)
    );
    let changed_head = PullRequestBaseHistory {
        head_sha: "cccccccccccccccccccccccccccccccccccccccc".to_owned(),
        ..after
    };
    assert_eq!(
        new_retarget_event(&before, &changed_head, "main", "sea/integration"),
        Err(WorkflowRunError::Malformed)
    );
    Ok(())
}
