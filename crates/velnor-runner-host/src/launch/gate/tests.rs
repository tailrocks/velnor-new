// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeSet, VecDeque};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
    mpsc,
};
use std::time::Duration;

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use super::{GithubObservation, decide, evaluate_rows, hold, scale_sets_to_observe};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
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

fn row(id: i64, state: IntentState) -> IntentRow {
    let mut row = done_row_with_runner("77");
    row.id = id;
    row.state = state;
    row.github_runner_id = None;
    row
}

fn lines(rows: &[IntentRow], docker: &[String], github: &GithubObservation) -> Vec<String> {
    hold::hold_lines(rows, docker, &[], github)
}

#[test]
fn a_docker_confirmed_absent_container_is_held_as_absent_not_unconfirmed() {
    let mut done = row(3, IntentState::Done);
    done.docker_id = Some("3f12ab99c0de5566".to_owned());
    let absent = ["3f12ab99c0de5566".to_owned()];
    let github = GithubObservation::listed(Ok(Vec::new()));
    let out = hold::hold_lines(&[done], &[], &absent, &github);
    assert_eq!(out.len(), 1);
    assert!(out[0].contains("why=\"container absent, cleanup not proven\""));
}

#[test]
fn a_pending_row_is_held_as_state_unsettled() {
    let rows = [row(7, IntentState::Pending)];
    let out = lines(&rows, &[], &GithubObservation::listed(Ok(Vec::new())));
    assert_eq!(out.len(), 1);
    assert!(out[0].starts_with("legacy_hold row=7 state=pending "));
    assert!(out[0].contains("why=\"launch state unsettled\""));
}

#[test]
fn a_done_row_with_a_gone_container_is_held_as_container_not_confirmed_gone() {
    let mut done = row(3, IntentState::Done);
    done.docker_id = Some("3f12ab99c0de5566".to_owned());
    let out = lines(&[done], &[], &GithubObservation::listed(Ok(Vec::new())));
    assert_eq!(out.len(), 1);
    assert!(out[0].contains("docker=3f12ab99c0de "));
    assert!(out[0].contains("why=\"container not confirmed gone\""));
}

#[test]
fn a_listed_runner_is_held_as_runner_still_listed() {
    let rows = [done_row_with_runner("77")];
    let github = GithubObservation::listed(Ok(vec!["77".to_owned()]));
    let out = lines(&rows, &[], &github);
    assert_eq!(out.len(), 1);
    assert!(out[0].contains("github_runner=77"));
    assert!(out[0].contains("why=\"runner still listed\""));
}

#[test]
fn an_unobservable_scale_set_names_why_it_could_not_be_observed() {
    let rows = [done_row_with_runner("77")];
    for reason in [
        "connection removed",
        "keychain secret missing",
        "scale set unobservable",
    ] {
        let github = GithubObservation::failed(reason, &BTreeSet::from([10]));
        let out = lines(&rows, &[], &github);
        assert_eq!(out.len(), 1);
        assert!(
            out[0].contains(&format!("why=\"{reason}\"")),
            "{reason}: {out:?}"
        );
    }
}

#[test]
fn a_released_row_and_a_clean_row_produce_no_line() {
    let mut proven = row(1, IntentState::Pending);
    proven.cleanup_proven = true;
    let clean = row(2, IntentState::Done);
    let out = lines(
        &[proven, clean],
        &[],
        &GithubObservation::listed(Ok(Vec::new())),
    );
    assert_eq!(out, Vec::<String>::new());
}

struct Blocked {
    release: mpsc::Receiver<()>,
}

impl Transport for Blocked {
    fn exchange(&mut self, _request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.release
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| TransportFail::Timeout)?;
        Ok(Exchange {
            status: 200,
            body: runners_page(&[]).into_bytes(),
        })
    }
}

/// The retry loop that drives the gate must keep ticking while GitHub blocks. On a single-thread
/// runtime a blocking call made inline would starve the ticker; `spawn_blocking` does not.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn the_thirty_second_retry_keeps_ticking_while_the_github_observe_blocks() -> TestResult {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("docker.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket)?;
    let docker = crate::docker_client::connect_unix(&socket.to_string_lossy())?;
    let (release, receive) = mpsc::channel();
    let rows = vec![done_row_with_runner("77")];
    let observe = evaluate_rows(rows, &docker, move |rows| {
        let mut transport = Blocked { release: receive };
        GithubObservation::listed(scale_sets_to_observe(rows).and_then(|sets| {
            let mut ids = Vec::new();
            for set in sets {
                ids.extend(list_runner_ids(&mut transport, "admin-token", set)?);
            }
            Ok(ids)
        }))
    });
    // Paused time does not auto-advance while a blocking task is in flight, so the test drives
    // the clock by hand. The ticker stands in for the controller loop's 30 s retry.
    let ticks = Arc::new(AtomicU32::new(0));
    let ticker = tokio::spawn({
        let ticks = Arc::clone(&ticks);
        async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                ticks.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    let mut observe = std::pin::pin!(observe);
    let mut released = false;
    let report = loop {
        tokio::select! {
            report = &mut observe => break report?,
            () = async {
                tokio::time::advance(Duration::from_secs(30)).await;
                tokio::task::yield_now().await;
            } => {
                if !released && ticks.load(Ordering::SeqCst) >= 3 {
                    release.send(())?;
                    released = true;
                }
            }
        }
    };
    ticker.abort();
    let ticks = ticks.load(Ordering::SeqCst);
    assert!(
        ticks >= 3,
        "ticker starved while the observe blocked: {ticks}"
    );
    assert!(matches!(report.decision, Reconcile::Advertise { .. }));
    Ok(())
}
