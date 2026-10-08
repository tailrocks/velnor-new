use std::collections::BTreeMap;

use velnor_runner_host::worker::{OwnedDockerResource, OwnedDockerResourceKind};
use velnor_runner_host::{IntentRow, IntentState};
use velnor_runner_journal::journal::{LaunchEffectState, RunnerStartIntent};

use super::inventory_matches_rows;

const WORKER: &str = "worker-a";

#[test]
fn complete_held_generation_matches_only_exact_owned_resources() {
    let row = held_row(1);
    let resources = complete_resources();

    assert!(inventory_matches_rows(&resources, &[row]));
}

#[test]
fn missing_expected_volume_keeps_capacity_occupied() {
    let row = held_row(1);
    let mut resources = complete_resources();
    resources.retain(|resource| resource.id_or_name != format!("{WORKER}-tmp"));

    assert!(!inventory_matches_rows(&resources, &[row]));
}

#[test]
fn untracked_owned_resource_keeps_capacity_occupied() {
    let row = held_row(1);
    let mut resources = complete_resources();
    resources.push(resource(
        OwnedDockerResourceKind::Volume,
        "untracked-volume",
        WORKER,
        "work",
        None,
        vec!["untracked-volume"],
    ));

    assert!(!inventory_matches_rows(&resources, &[row]));
}

#[test]
fn container_without_exact_worker_volume_label_is_rejected() {
    let row = held_row(1);
    let mut resources = complete_resources();
    let runner = resources
        .iter_mut()
        .find(|resource| resource.role == "runner")
        .expect("runner fixture exists");
    runner.labels.remove("velnor.volume");

    assert!(!inventory_matches_rows(&resources, &[row]));
}

#[test]
fn ambiguous_duplicate_generation_rows_are_rejected() {
    let row = held_row(1);
    let mut duplicate = row.clone();
    duplicate.id = 2;

    assert!(!inventory_matches_rows(
        &complete_resources(),
        &[row, duplicate]
    ));
}

fn held_row(id: i64) -> IntentRow {
    IntentRow {
        id,
        kind: "launch".to_owned(),
        subject: "fixture".to_owned(),
        state: IntentState::Done,
        launch_effect: LaunchEffectState::MayHaveEffect,
        docker_id: Some("runner-id".to_owned()),
        dind_id: Some("dind-id".to_owned()),
        worker_volume: Some(WORKER.to_owned()),
        github_runner_id: None,
        message_id: Some(4),
        runner_request_id: Some(9),
        requested_workflow_run_id: Some(15),
        requested_job_id: Some("opaque-job".to_owned()),
        runner_name: Some("v1".to_owned()),
        observed_job_id: None,
        observed_workflow_run_id: None,
        observed_actions_attempt: None,
        observed_actions_job_id: None,
        observed_actions_conclusion: None,
        remote_terminal: false,
        cleanup_proven: false,
        outer_network_name: Some("worker-a-outer".to_owned()),
        outer_network_id: Some("network-id".to_owned()),
        runner_start_intent: RunnerStartIntent::MayHaveStarted,
    }
}

fn complete_resources() -> Vec<OwnedDockerResource> {
    let mut resources = vec![
        resource(
            OwnedDockerResourceKind::Container,
            "runner-id",
            WORKER,
            "runner",
            Some(WORKER),
            vec!["/v1"],
        ),
        resource(
            OwnedDockerResourceKind::Container,
            "dind-id",
            WORKER,
            "dind",
            Some(WORKER),
            vec!["/worker-a-dind"],
        ),
        resource(
            OwnedDockerResourceKind::Network,
            "network-id",
            WORKER,
            "outer-network",
            Some(WORKER),
            vec!["worker-a-outer"],
        ),
    ];
    for (name, role) in [
        (WORKER.to_owned(), "socket"),
        (format!("{WORKER}-work"), "work"),
        (format!("{WORKER}-externals"), "externals"),
        (format!("{WORKER}-docker"), "dind-data"),
        (format!("{WORKER}-home"), "home-state"),
        (format!("{WORKER}-tmp"), "runner-temp"),
    ] {
        resources.push(resource(
            OwnedDockerResourceKind::Volume,
            &name,
            WORKER,
            role,
            None,
            vec![&name],
        ));
    }
    resources
}

fn resource(
    kind: OwnedDockerResourceKind,
    id_or_name: &str,
    worker: &str,
    role: &str,
    volume: Option<&str>,
    names: Vec<&str>,
) -> OwnedDockerResource {
    let mut labels = BTreeMap::from([
        ("velnor.worker".to_owned(), worker.to_owned()),
        ("velnor.role".to_owned(), role.to_owned()),
    ]);
    if let Some(volume) = volume {
        labels.insert("velnor.volume".to_owned(), volume.to_owned());
    }
    OwnedDockerResource {
        kind,
        id_or_name: id_or_name.to_owned(),
        names: names.into_iter().map(str::to_owned).collect(),
        worker: worker.to_owned(),
        role: role.to_owned(),
        labels,
    }
}
