// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use super::{decide, scale_sets_to_observe};
use crate::journal::IntentState;
use crate::reconcile::{IntentRow, Reconcile};
use crate::scale_set::list_runner_ids;

struct Script {
    replies: VecDeque<(u16, String)>,
    seen: Vec<(Method, String)>,
}

impl Script {
    fn new(replies: Vec<(u16, String)>) -> Self {
        Self {
            replies: VecDeque::from(replies),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push((request.method, request.path.clone()));
        let (status, body) = self.replies.pop_front().ok_or(TransportFail::Reset)?;
        Ok(Exchange {
            status,
            body: body.into_bytes(),
        })
    }
}

fn runners_page(ids: &[i64]) -> String {
    let value: Vec<String> = ids
        .iter()
        .map(|id| format!(r#"{{"id":{id},"name":"runner-{id}","runnerScaleSetId":10}}"#))
        .collect();
    format!(r#"{{"count":{},"value":[{}]}}"#, ids.len(), value.join(","))
}

fn done_row_with_runner(runner_id: &str) -> IntentRow {
    IntentRow {
        id: 1,
        kind: "launch".to_owned(),
        subject: "subject".to_owned(),
        state: IntentState::Done,
        docker_id: None,
        dind_id: None,
        worker_volume: None,
        scale_set_id: Some(10),
        request_id: None,
        runner_name: None,
        docker_engine_id: None,
        launch_phase: None,
        github_runner_id: Some(runner_id.to_owned()),
        cleanup_proven: false,
        launch_id: None,
        assignment_key: None,
        seed_generation_id: None,
        acquire_attempted: false,
        acquire_resolved: false,
        acquired: false,
        jit_requested: false,
        runner_completed: false,
    }
}

fn gate(rows: &[IntentRow], replies: Vec<(u16, String)>) -> (Reconcile, Script) {
    let mut transport = Script::new(replies);
    let github = scale_sets_to_observe(rows).and_then(|sets| {
        let mut ids = Vec::new();
        for set in sets {
            ids.extend(list_runner_ids(&mut transport, "admin-token", set)?);
        }
        Ok(ids)
    });
    (decide(rows, &[], github), transport)
}

#[test]
fn runner_that_github_still_lists_blocks_release() {
    let rows = [done_row_with_runner("77")];
    let (decision, transport) = gate(&rows, vec![(200, runners_page(&[77]))]);
    assert_eq!(transport.seen.len(), 1);
    assert!(matches!(decision, Reconcile::Hold { occupied: 1, .. }));
}

#[test]
fn release_passes_once_github_no_longer_lists_the_runner() {
    let rows = [done_row_with_runner("77")];
    let (decision, _) = gate(&rows, vec![(200, runners_page(&[]))]);
    assert!(matches!(decision, Reconcile::Advertise { .. }));
}

#[test]
fn runner_of_another_scale_set_does_not_count() {
    let rows = [done_row_with_runner("77")];
    let other = r#"{"count":1,"value":[{"id":77,"name":"r","runnerScaleSetId":99}]}"#;
    let (decision, _) = gate(&rows, vec![(200, other.to_owned())]);
    assert!(matches!(decision, Reconcile::Advertise { .. }));
}

#[test]
fn github_observation_error_keeps_gate_closed() {
    let rows = [done_row_with_runner("77")];
    for reply in [
        (500, String::new()),
        (403, String::new()),
        (200, "not json".to_owned()),
        (200, r#"{"count":2,"value":[]}"#.to_owned()),
    ] {
        let (decision, _) = gate(&rows, vec![reply]);
        assert!(matches!(decision, Reconcile::Hold { occupied: 1, .. }));
    }
    let (decision, _) = gate(&rows, vec![]);
    assert!(matches!(decision, Reconcile::Hold { occupied: 1, .. }));
}

#[test]
fn truncated_page_where_count_exceeds_entries_holds_the_gate() {
    let rows = [done_row_with_runner("77")];
    let page = r#"{"count":3,"value":[{"id":1,"name":"a","runnerScaleSetId":10},{"id":2,"name":"b","runnerScaleSetId":10}]}"#;
    let (decision, transport) = gate(&rows, vec![(200, page.to_owned())]);
    assert_eq!(transport.seen.len(), 1);
    assert!(matches!(decision, Reconcile::Hold { occupied: 1, .. }));
}

#[test]
fn runner_without_a_scale_set_cannot_be_observed_and_closes_gate() {
    let mut row = done_row_with_runner("77");
    row.scale_set_id = None;
    let (decision, transport) = gate(&[row], vec![]);
    assert_eq!(transport.seen.len(), 0);
    assert!(matches!(decision, Reconcile::Hold { .. }));
}

#[test]
fn rows_without_runner_ids_need_no_github_call() {
    let mut row = done_row_with_runner("77");
    row.github_runner_id = None;
    let (decision, transport) = gate(&[row], vec![]);
    assert_eq!(transport.seen.len(), 0);
    assert!(matches!(decision, Reconcile::Advertise { .. }));
}
