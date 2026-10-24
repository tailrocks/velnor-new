use super::super::github::{RunRepository, WorkflowJob};
use super::*;
use serde_json::{Value, json};

fn workflow() -> WorkflowMetadata {
    WorkflowMetadata {
        state: "active".to_owned(),
        id: 7,
        name: WORKFLOW_DISPLAY_NAME.to_owned(),
        path: CI_WORKFLOW_PATH.to_owned(),
    }
}

pub(super) fn source() -> RunSource<'static> {
    RunSource {
        repository_id: 9,
        sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        branch: "main",
    }
}

pub(super) fn run_json() -> Value {
    json!({"id":22,"run_attempt":3,"run_number":12,"workflow_id":7,"name":"CI",
        "path":".github/workflows/ci.yml","head_sha":source().sha,"head_branch":"main",
        "event":"push","status":"completed","conclusion":"success",
        "repository":{"id":9,"full_name":"example/app"},"head_repository":{"id":9,"full_name":"example/app"}})
}

fn parse(value: Value) -> WorkflowRunResponse {
    serde_json::from_value(value).expect("workflow run fixture")
}

#[test]
fn native_ci_reauthentication_binds_every_run_workflow_and_repository_field() {
    assert!(ci_run_is_success(
        &parse(run_json()),
        22,
        3,
        source(),
        &workflow()
    ));
    for (pointer, replacement) in [
        ("/id", json!(23)),
        ("/run_attempt", json!(2)),
        ("/workflow_id", json!(8)),
        ("/name", json!("Other CI")),
        ("/path", json!(".github/workflows/other.yml")),
        ("/head_sha", json!("b".repeat(40))),
        ("/head_branch", json!("other")),
        ("/event", json!("workflow_run")),
        ("/event", json!("pull_request")),
        ("/status", json!("in_progress")),
        ("/conclusion", json!("failure")),
        ("/repository/id", json!(10)),
        ("/head_repository/id", json!(10)),
        ("/head_repository", Value::Null),
        ("/workflow_id", Value::Null),
    ] {
        let mut value = run_json();
        *value.pointer_mut(pointer).expect("mutation field") = replacement;
        assert!(
            !ci_run_is_success(&parse(value), 22, 3, source(), &workflow()),
            "accepted {pointer}"
        );
    }
    let renamed = RunSource {
        repository_id: 10,
        ..source()
    };
    assert!(!ci_run_is_success(
        &parse(run_json()),
        22,
        3,
        renamed,
        &workflow()
    ));
}

#[test]
fn latest_source_ci_selects_newest_attempt_without_rewriting_original_identity() {
    let original = parse(run_json());
    let mut latest = run_json();
    latest["run_attempt"] = json!(4);
    let selected = latest_ci_run(vec![parse(latest)], &workflow(), source(), "example/app")
        .expect("latest run");
    assert!(ci_run_is_success(&original, 22, 3, source(), &workflow()));
    assert!(!ci_run_is_success(&selected, 22, 3, source(), &workflow()));
    assert!(ci_run_is_success(&selected, 22, 4, source(), &workflow()));
    assert_eq!(
        original.run_attempt,
        Some(3),
        "retained receipt CI attempt is immutable"
    );
    let mut pending = run_json();
    pending["id"] = json!(23);
    pending["run_number"] = json!(13);
    pending["status"] = json!("in_progress");
    pending["conclusion"] = Value::Null;
    let selected = latest_ci_run(
        vec![original, parse(pending)],
        &workflow(),
        source(),
        "example/app",
    )
    .expect("latest pending run");
    assert_eq!(selected.id, Some(23));
    assert!(!ci_run_is_success(&selected, 23, 3, source(), &workflow()));

    let mut failed = run_json();
    failed["id"] = json!(24);
    failed["run_number"] = json!(14);
    failed["conclusion"] = json!("failure");
    let selected = latest_ci_run(
        vec![parse(run_json()), parse(failed)],
        &workflow(),
        source(),
        "example/app",
    )
    .expect("latest failed run");
    assert_eq!(selected.id, Some(24));
    assert!(!ci_run_is_success(&selected, 24, 3, source(), &workflow()));
}

#[test]
fn latest_source_ci_rejects_missing_ambiguous_and_foreign_inventory() {
    assert!(latest_ci_run(vec![], &workflow(), source(), "example/app").is_err());
    let mut duplicate = run_json();
    duplicate["id"] = json!(23);
    assert!(
        latest_ci_run(
            vec![parse(run_json()), parse(duplicate)],
            &workflow(),
            source(),
            "example/app"
        )
        .is_err()
    );
    for (field, replacement) in [
        ("run_number", Value::Null),
        ("run_number", json!(0)),
        ("id", json!(0)),
        ("run_attempt", json!(0)),
        ("event", json!("workflow_run")),
    ] {
        let mut value = run_json();
        value[field] = replacement;
        assert!(
            latest_ci_run(vec![parse(value)], &workflow(), source(), "example/app").is_err(),
            "accepted {field}"
        );
    }
}

fn required() -> WorkflowJob {
    WorkflowJob {
        id: 0,
        run_id: 22,
        name: REQUIRED_DISPLAY_NAME.to_owned(),
        head_sha: Some(source().sha.to_owned()),
        status: Some("completed".to_owned()),
        conclusion: Some("success".to_owned()),
    }
}

#[test]
fn native_ci_requires_unique_successful_required_job_for_exact_source() {
    assert!(
        require_required_job(
            &WorkflowJobList {
                jobs: vec![required()]
            },
            22,
            source().sha
        )
        .is_ok()
    );
    assert!(require_required_job(&WorkflowJobList { jobs: vec![] }, 22, source().sha).is_err());
    assert!(
        require_required_job(
            &WorkflowJobList {
                jobs: vec![required(), required()]
            },
            22,
            source().sha
        )
        .is_err()
    );
    let mut wrong_run = required();
    wrong_run.run_id = 23;
    let mut wrong_sha = required();
    wrong_sha.head_sha = Some("b".repeat(40));
    let mut pending = required();
    pending.status = Some("in_progress".to_owned());
    let mut failed = required();
    failed.conclusion = Some("failure".to_owned());
    for job in [wrong_run, wrong_sha, pending, failed] {
        assert!(
            require_required_job(&WorkflowJobList { jobs: vec![job] }, 22, source().sha).is_err()
        );
    }
}

fn jobs_page(total_count: usize, ids: impl IntoIterator<Item = u64>) -> WorkflowJobsPage {
    WorkflowJobsPage::from_ids(total_count, 22, source().sha, ids)
}

#[test]
fn workflow_job_inventory_requires_stable_complete_pagination_and_unique_ids() {
    let mut complete = WorkflowJobsAccumulator::default();
    assert!(
        !complete
            .admit_page(jobs_page(128, 1..=100))
            .expect("first page")
    );
    assert!(
        complete
            .admit_page(jobs_page(128, 101..=128))
            .expect("final page")
    );
    assert_eq!(complete.into_list().jobs.len(), 128);

    let mut changed_total = WorkflowJobsAccumulator::default();
    assert!(
        !changed_total
            .admit_page(jobs_page(128, 1..=100))
            .expect("first page")
    );
    assert!(changed_total.admit_page(jobs_page(129, 101..=128)).is_err());

    let mut short = WorkflowJobsAccumulator::default();
    assert!(short.admit_page(jobs_page(128, 1..=99)).is_err());
    let mut duplicate = WorkflowJobsAccumulator::default();
    assert!(duplicate.admit_page(jobs_page(2, [17, 17])).is_err());
    let mut zero = WorkflowJobsAccumulator::default();
    assert!(zero.admit_page(jobs_page(1, [0])).is_err());
}

#[test]
fn producing_workflow_run_is_separate_from_upstream_push_ci() {
    let metadata = WorkflowMetadata {
        state: "active".to_owned(),
        id: 8,
        name: "Rust binary preview".to_owned(),
        path: ".github/workflows/binary-preview.yml".to_owned(),
    };
    let mut value = run_json();
    value["workflow_id"] = json!(8);
    value["name"] = json!(metadata.name);
    value["path"] = json!(metadata.path);
    value["event"] = json!("workflow_run");
    assert!(run_matches(
        &parse(value.clone()),
        &metadata,
        22,
        3,
        source(),
        "workflow_run"
    ));
    for event in ["push", "workflow_dispatch", "pull_request"] {
        value["event"] = json!(event);
        assert!(!run_matches(
            &parse(value.clone()),
            &metadata,
            22,
            3,
            source(),
            "workflow_run"
        ));
    }
    let mut foreign = parse(run_json());
    foreign.head_repository = Some(RunRepository {
        id: 10,
        full_name: Some("attacker/fork".to_owned()),
    });
    assert!(!ci_run_is_success(&foreign, 22, 3, source(), &workflow()));
}

#[test]
fn preview_source_a_and_workflow_authority_b_remain_distinct_identities() {
    let metadata = WorkflowMetadata {
        state: "active".to_owned(),
        id: 8,
        name: "Rust binary preview".to_owned(),
        path: ".github/workflows/binary-preview.yml".to_owned(),
    };
    let mut producer = run_json();
    producer["workflow_id"] = json!(8);
    producer["name"] = json!(metadata.name);
    producer["path"] = json!(metadata.path);
    producer["event"] = json!("workflow_run");
    producer["head_sha"] = json!("b".repeat(40));
    let authority = RunSource {
        sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ..source()
    };
    assert!(run_matches(
        &parse(producer.clone()),
        &metadata,
        22,
        3,
        authority,
        "workflow_run"
    ));
    assert!(!run_matches(
        &parse(producer),
        &metadata,
        22,
        3,
        source(),
        "workflow_run"
    ));
    assert!(ci_run_is_success(
        &parse(run_json()),
        22,
        3,
        source(),
        &workflow()
    ));
    assert!(!ci_run_is_success(
        &parse(run_json()),
        22,
        3,
        authority,
        &workflow()
    ));
}

#[test]
fn workflow_metadata_admission_requires_active_exact_canonical_identity() {
    assert!(
        validate_workflow_metadata(&workflow(), CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME).is_ok()
    );
    for field in ["id", "name", "path", "state"] {
        let mut metadata = workflow();
        match field {
            "id" => metadata.id = 0,
            "name" => metadata.name = "Other".to_owned(),
            "path" => metadata.path = ".github/workflows/other.yml".to_owned(),
            "state" => metadata.state = "disabled_manually".to_owned(),
            _ => unreachable!("closed fixture inventory"),
        }
        assert!(
            validate_workflow_metadata(&metadata, CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME).is_err(),
            "accepted {field}"
        );
    }
}

#[test]
fn completed_attempt_cache_has_full_source_and_workflow_identity() {
    let mut cache = RunIdentityCache::default();
    cache
        .admit_completed(cache::CompletedAttempt::new(22, 3, source(), &workflow()))
        .expect("admit");
    assert!(cache.contains(&cache::CompletedAttempt::new(22, 3, source(), &workflow())));
    for changed in [
        RunSource {
            repository_id: 10,
            ..source()
        },
        RunSource {
            sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ..source()
        },
        RunSource {
            branch: "other",
            ..source()
        },
    ] {
        assert!(!cache.contains(&cache::CompletedAttempt::new(22, 3, changed, &workflow())));
    }
    for (id, attempt) in [(23, 3), (22, 4)] {
        assert!(!cache.contains(&cache::CompletedAttempt::new(
            id,
            attempt,
            source(),
            &workflow()
        )));
    }
    let mut changed = workflow();
    changed.id = 8;
    assert!(!cache.contains(&cache::CompletedAttempt::new(22, 3, source(), &changed)));
    changed = workflow();
    changed.name = "Other CI".to_owned();
    assert!(!cache.contains(&cache::CompletedAttempt::new(22, 3, source(), &changed)));
    changed = workflow();
    changed.path = ".github/workflows/other.yml".to_owned();
    assert!(!cache.contains(&cache::CompletedAttempt::new(22, 3, source(), &changed)));
}
