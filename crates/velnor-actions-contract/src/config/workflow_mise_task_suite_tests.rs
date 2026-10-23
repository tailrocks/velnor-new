use super::named;
use crate::config::{
    GitHubEvidenceArtifactSource, GitHubEvidenceSnapshotInput, GitHubEvidenceWorkflowSource,
    MiseTaskSource, MiseTaskSuite, MiseTaskSuiteEvent, RustSuiteArtifact, RustSuiteArtifactWhen,
    VerificationRunner, WorkflowConfig,
};

fn suite(id: &str, events: Vec<MiseTaskSuiteEvent>) -> MiseTaskSuite {
    MiseTaskSuite {
        id: id.to_owned(),
        mise_task: format!("collect-{id}"),
        source: MiseTaskSource::default(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 15,
        workflow: "evidence".to_owned(),
        events,
        schedule: Vec::new(),
        artifacts: Vec::new(),
        evidence_snapshot: None,
    }
}

#[test]
fn task_suites_accept_only_explicit_push_and_schedule_events() {
    let mut config = named("CI");
    let mut scheduled = suite(
        "nightly",
        vec![MiseTaskSuiteEvent::Push, MiseTaskSuiteEvent::Schedule],
    );
    scheduled.schedule = vec!["0 3 * * *".to_owned()];
    scheduled.artifacts = vec![RustSuiteArtifact {
        name: "evidence".to_owned(),
        paths: vec!["target/evidence/**".to_owned()],
        when: RustSuiteArtifactWhen::Success,
        retention_days: 45,
    }];
    config.task_suites = vec![scheduled.clone()];
    assert!(config.validate("config.toml").is_ok());

    let mut bad = scheduled.clone();
    bad.events.clear();
    assert!(
        bad.validate("config.toml", "workflow.task_suites[0]")
            .is_err()
    );

    let mut bad = scheduled.clone();
    bad.events.push(MiseTaskSuiteEvent::Schedule);
    assert!(
        bad.validate("config.toml", "workflow.task_suites[0]")
            .is_err()
    );

    let mut bad = scheduled.clone();
    bad.schedule[0] = "not a cron".to_owned();
    assert!(
        bad.validate("config.toml", "workflow.task_suites[0]")
            .is_err()
    );

    let mut bad = scheduled.clone();
    bad.events = vec![MiseTaskSuiteEvent::Push];
    assert!(
        bad.validate("config.toml", "workflow.task_suites[0]")
            .is_err()
    );

    let mut bad = scheduled;
    bad.artifacts[0].name = "evidence-{shard}".to_owned();
    assert!(
        bad.validate("config.toml", "workflow.task_suites[0]")
            .is_err()
    );
}

#[test]
fn task_suite_artifact_requires_one_path_pattern_per_declaration() {
    let mut task = suite("evidence", vec![MiseTaskSuiteEvent::Push]);
    task.artifacts = vec![RustSuiteArtifact {
        name: "evidence".to_owned(),
        paths: vec![
            "target/evidence/manifest.json".to_owned(),
            "target/evidence/push-event.json".to_owned(),
        ],
        when: RustSuiteArtifactWhen::Success,
        retention_days: 45,
    }];

    let error = task
        .validate("config.toml", "workflow.task_suites[0]")
        .expect_err("multiple patterns could hide one missing output");
    assert!(
        error
            .to_string()
            .contains("task_suite_artifact_requires_one_path_pattern")
    );
}

#[test]
fn task_suite_inventory_is_sorted_unique_and_generator_owned() {
    let mut config = named("CI");
    config.task_suites = vec![suite("alpha", vec![MiseTaskSuiteEvent::Push])];
    assert!(config.validate("config.toml").is_ok());

    config.preserve_workflows = vec![".github/workflows/evidence.yml".to_owned()];
    assert!(config.validate("config.toml").is_err());

    config.preserve_workflows.clear();
    config.task_suites = vec![
        suite("beta", vec![MiseTaskSuiteEvent::Push]),
        suite("alpha", vec![MiseTaskSuiteEvent::Push]),
    ];
    assert!(config.validate("config.toml").is_err());
}

#[test]
fn task_suite_rejects_unsafe_task_source_slug_timeout_and_artifact() {
    let invalid = |edit: fn(&mut MiseTaskSuite)| {
        let mut suite = suite("evidence", vec![MiseTaskSuiteEvent::Push]);
        edit(&mut suite);
        assert!(
            suite
                .validate("config.toml", "workflow.task_suites[0]")
                .is_err()
        );
    };

    invalid(|suite| suite.id = "../evidence".to_owned());
    invalid(|suite| suite.mise_task = "collect; echo unsafe".to_owned());
    invalid(|suite| suite.source.working_directory = "../outside".to_owned());
    invalid(|suite| suite.workflow = "../evidence".to_owned());
    invalid(|suite| suite.timeout_minutes = 0);
    invalid(|suite| {
        suite.artifacts.push(RustSuiteArtifact {
            name: "evidence".to_owned(),
            paths: vec!["../private.json".to_owned()],
            when: RustSuiteArtifactWhen::Success,
            retention_days: 0,
        });
    });
}

#[test]
fn task_suite_policy_is_closed_to_credentials_and_unknown_fields() {
    let mut config = named("CI");
    config.task_suites = vec![suite("evidence", vec![MiseTaskSuiteEvent::Push])];
    let mut value = serde_json::to_value(config).expect("serialize config");
    value["task_suites"][0]["credentials"] = serde_json::json!("github-actions-read");
    assert!(serde_json::from_value::<WorkflowConfig>(value).is_err());
}

#[test]
fn snapshot_contract_requires_schedule_collector_and_exact_push_producer_artifacts() {
    let mut config = named("CI");
    let mut collector = suite("nightly", vec![MiseTaskSuiteEvent::Schedule]);
    collector.schedule = vec!["0 3 * * *".to_owned()];
    collector.evidence_snapshot = Some(GitHubEvidenceSnapshotInput {
        window_days: 31,
        workflow_sources: vec![
            GitHubEvidenceWorkflowSource::PrimaryCi,
            GitHubEvidenceWorkflowSource::TaskSuite {
                suite_id: "push-head".to_owned(),
            },
        ],
        artifact_sources: vec![GitHubEvidenceArtifactSource {
            suite_id: "push-head".to_owned(),
            artifact_name: "event".to_owned(),
        }],
    });
    let mut producer = suite("push-head", vec![MiseTaskSuiteEvent::Push]);
    producer.artifacts = vec![RustSuiteArtifact {
        name: "event".to_owned(),
        paths: vec!["target/evidence/event.json".to_owned()],
        when: RustSuiteArtifactWhen::Always,
        retention_days: 45,
    }];
    config.task_suites = vec![collector.clone(), producer.clone()];
    assert!(config.validate("config.toml").is_ok());

    let mut bad = config.clone();
    bad.task_suites[0]
        .evidence_snapshot
        .as_mut()
        .expect("test config has a snapshot declaration")
        .workflow_sources[1] = GitHubEvidenceWorkflowSource::TaskSuite {
        suite_id: "missing".to_owned(),
    };
    assert!(bad.validate("config.toml").is_err());

    let mut bad = config.clone();
    bad.task_suites[0]
        .evidence_snapshot
        .as_mut()
        .expect("test config has a snapshot declaration")
        .artifact_sources[0]
        .artifact_name = "undeclared".to_owned();
    assert!(bad.validate("config.toml").is_err());

    let mut bad = config;
    bad.task_suites[0].events.push(MiseTaskSuiteEvent::Push);
    assert!(bad.validate("config.toml").is_err());
}

#[test]
fn snapshot_contract_rejects_unbounded_unsorted_and_open_fields() {
    let mut declaration = GitHubEvidenceSnapshotInput {
        window_days: 32,
        workflow_sources: vec![GitHubEvidenceWorkflowSource::PrimaryCi],
        artifact_sources: vec![GitHubEvidenceArtifactSource {
            suite_id: "producer".to_owned(),
            artifact_name: "event".to_owned(),
        }],
    };
    assert!(declaration.validate("config.toml", "snapshot").is_err());
    declaration.window_days = 7;
    declaration
        .workflow_sources
        .push(GitHubEvidenceWorkflowSource::PrimaryCi);
    assert!(declaration.validate("config.toml", "snapshot").is_err());
    let mut value = serde_json::to_value(declaration).expect("serialize snapshot declaration");
    value["raw_url"] = serde_json::json!("https://example.invalid");
    assert!(serde_json::from_value::<GitHubEvidenceSnapshotInput>(value).is_err());
}
