use super::*;
use std::error::Error;

use velnor_actions_contract::{
    GitHubEvidenceArtifactSource, GitHubEvidenceSnapshotInput, GitHubEvidenceWorkflowSource,
    MiseTaskSource, ReleaseManifest, RustSuiteArtifact, RustSuiteArtifactWhen, TargetRecord,
    VerificationRunner,
};

type TestResult = Result<(), Box<dyn Error>>;

fn setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{}", "b".repeat(40)),
        version: "2026.10.17".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn policy(id: &str, events: Vec<MiseTaskSuiteEvent>, schedule: Vec<String>) -> MiseTaskSuitePolicy {
    MiseTaskSuitePolicy {
        suite: MiseTaskSuite {
            id: id.to_owned(),
            mise_task: format!("collect-{id}"),
            source: MiseTaskSource::default(),
            runner: VerificationRunner::LinuxX64,
            timeout_minutes: 20,
            workflow: "evidence".to_owned(),
            events,
            schedule,
            artifacts: Vec::new(),
            evidence_snapshot: None,
        },
        runner_label: "ubuntu-26.04".to_owned(),
        mise_setup: setup(),
        selected_tools: Vec::new(),
        mise_config_sha256: Some("c".repeat(64)),
        mise_lock_sha256: None,
        rust_toolchain_sha256: None,
        evidence_fetch_spec: None,
        evidence_helper: None,
    }
}

#[test]
fn no_evidence_push_workflow_matches_pre_feature_bytes() -> TestResult {
    let mut push = policy("push-head", vec![MiseTaskSuiteEvent::Push], Vec::new());
    push.suite.artifacts.push(RustSuiteArtifact {
        name: "ledger".to_owned(),
        paths: vec!["target/evidence/ledger.jsonl".to_owned()],
        when: RustSuiteArtifactWhen::Success,
        retention_days: 45,
    });
    let mut render_spec = spec(vec![push]);
    render_spec.generator_version = "0.1.0".to_owned();
    let rendered = render_mise_task_suites(&render_spec)?;
    assert_eq!(
        rendered[0].bytes.as_bytes(),
        include_bytes!("mise_task_suites_legacy_push.golden.yml")
    );
    Ok(())
}

fn spec(suites: Vec<MiseTaskSuitePolicy>) -> MiseTaskSuitesRenderSpec {
    MiseTaskSuitesRenderSpec {
        suites,
        default_branch: "main".to_owned(),
        checkout_uses: crate::steps::CHECKOUT_USES.to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

fn evidence_helper() -> EvidenceHelperProvenance {
    let version = env!("CARGO_PKG_VERSION");
    let manifest = evidence_helper_manifest();
    EvidenceHelperProvenance::from_release_manifest(&manifest, "ubuntu-26.04", version)
        .expect("verified evidence helper")
}

fn evidence_suites() -> Vec<MiseTaskSuitePolicy> {
    let mut collector = policy(
        "collector",
        vec![MiseTaskSuiteEvent::Schedule],
        vec!["0 3 * * *".to_owned()],
    );
    let input = GitHubEvidenceSnapshotInput {
        window_days: 7,
        workflow_sources: vec![
            GitHubEvidenceWorkflowSource::PrimaryCi,
            GitHubEvidenceWorkflowSource::TaskSuite {
                suite_id: "push-head".to_owned(),
            },
        ],
        artifact_sources: vec![GitHubEvidenceArtifactSource {
            suite_id: "push-head".to_owned(),
            artifact_name: "ledger".to_owned(),
        }],
    };
    let mut producer = policy("push-head", vec![MiseTaskSuiteEvent::Push], Vec::new());
    producer.suite.artifacts.push(RustSuiteArtifact {
        name: "ledger".to_owned(),
        paths: vec!["target/evidence/push-ledger.jsonl".to_owned()],
        when: RustSuiteArtifactWhen::Always,
        retention_days: 45,
    });
    let inventory = [collector.suite.clone(), producer.suite.clone()];
    collector.evidence_fetch_spec = Some(
        input
            .resolve_spec(&inventory, &collector.suite.id, "main")
            .expect("spec"),
    );
    collector.suite.evidence_snapshot = Some(input);
    collector.evidence_helper = Some(evidence_helper());
    vec![collector, producer]
}

#[test]
fn empty_task_suite_inventory_emits_no_generated_files() {
    let rendered = render_mise_task_suites(&spec(Vec::new())).expect("empty inventory");
    assert_eq!(rendered, Vec::<crate::RenderedFile>::new());
}

#[test]
fn mixed_event_companion_gates_jobs_and_has_no_concurrency() -> TestResult {
    let mut nightly = policy(
        "nightly",
        vec![MiseTaskSuiteEvent::Schedule],
        vec!["0 3 * * *".to_owned()],
    );
    let mut push = policy("push-head", vec![MiseTaskSuiteEvent::Push], Vec::new());
    nightly.suite.artifacts.push(RustSuiteArtifact {
        name: "manifest".to_owned(),
        paths: vec!["target/evidence/manifest.json".to_owned()],
        when: RustSuiteArtifactWhen::Success,
        retention_days: 45,
    });
    push.suite.artifacts.push(RustSuiteArtifact {
        name: "ledger".to_owned(),
        paths: vec!["target/evidence/push-ledger.jsonl".to_owned()],
        when: RustSuiteArtifactWhen::Success,
        retention_days: 45,
    });
    let rendered = render_mise_task_suites(&spec(vec![nightly, push]))?;
    assert_eq!(rendered.len(), 1);
    let yaml = rendered[0].bytes.clone();
    assert!(yaml.contains("push:"));
    assert!(yaml.contains("branches:"));
    assert!(yaml.contains("- main"));
    assert!(yaml.contains("schedule:"));
    assert!(yaml.contains("cron: 0 3 * * *"));
    assert!(yaml.contains("github.event_name == 'push' && github.ref == 'refs/heads/main'"));
    assert!(
        yaml.contains("github.event_name == 'schedule' && (github.event.schedule == '0 3 * * *')")
    );
    assert!(!yaml.contains("pull_request:"));
    assert!(!yaml.contains("workflow_dispatch:"));
    assert!(!yaml.contains("merge_group:"));
    assert!(!yaml.contains("concurrency:"));
    assert!(yaml.contains("ref: ${{ github.sha }}"));
    assert!(yaml.contains("fetch-depth:"), "{yaml}");
    assert!(yaml.contains("persist-credentials:"), "{yaml}");
    assert!(yaml.contains("CARGO_BUILD_JOBS: \"2\""), "{yaml}");
    assert!(yaml.contains("NEXTEST_TEST_THREADS: \"2\""), "{yaml}");
    assert!(yaml.contains("GITHUB_TOKEN: \"\""), "{yaml}");
    assert!(yaml.contains("GH_TOKEN: \"\""), "{yaml}");
    assert!(!yaml.contains("actions: read"));
    assert!(!yaml.contains("GH_TOKEN: ${{ github.token }}"));
    assert!(!yaml.contains("GITHUB_TOKEN: ${{ github.token }}"));
    Ok(())
}

fn evidence_helper_manifest() -> ReleaseManifest {
    let version = env!("CARGO_PKG_VERSION");
    ReleaseManifest {
        schema: ReleaseManifest::SCHEMA,
        version: version.to_owned(),
        repository: velnor_actions_contract::EXPECTED_REPOSITORY.to_owned(),
        commit: "b".repeat(40),
        targets: velnor_actions_contract::SUPPORTED_TARGETS
            .iter()
            .map(|target| TargetRecord {
                target: (*target).to_owned(),
                artifact: format!(
                    "https://github.com/{}/releases/download/v{version}/velnor-actions-{version}-{target}",
                    velnor_actions_contract::EXPECTED_REPOSITORY
                ),
                sha256: "a".repeat(64),
            })
            .collect(),
    }
}

#[test]
fn push_attempt_artifact_is_required_and_attempt_qualified() -> TestResult {
    let mut push = policy("push-head", vec![MiseTaskSuiteEvent::Push], Vec::new());
    push.suite.artifacts.push(RustSuiteArtifact {
        name: "ledger".to_owned(),
        paths: vec!["target/evidence/push-ledger.jsonl".to_owned()],
        when: RustSuiteArtifactWhen::Success,
        retention_days: 45,
    });
    for _ in 0..3 {
        let rendered = render_mise_task_suites(&spec(vec![push.clone()]))?;
        let yaml = rendered[0].bytes.clone();
        assert!(!yaml.contains("concurrency:"));
        assert!(yaml.contains(
            "velnor-mise-task-suite-9-push-head-artifact-6-ledger-${{ github.run_id }}-attempt-${{ github.run_attempt }}"
        ));
        assert!(yaml.contains("if-no-files-found: error"));
        assert!(
            yaml.contains("retention-days:") && yaml.contains("45"),
            "{yaml}"
        );
        assert!(yaml.contains("if: success()"));
    }
    Ok(())
}

#[test]
fn each_artifact_path_has_an_independent_fail_closed_upload() -> TestResult {
    let mut push = policy("push-head", vec![MiseTaskSuiteEvent::Push], Vec::new());
    push.suite.artifacts = vec![
        RustSuiteArtifact {
            name: "manifest".to_owned(),
            paths: vec!["target/evidence/manifest.json".to_owned()],
            when: RustSuiteArtifactWhen::Success,
            retention_days: 45,
        },
        RustSuiteArtifact {
            name: "push-event".to_owned(),
            paths: vec!["target/evidence/push-event.json".to_owned()],
            when: RustSuiteArtifactWhen::Success,
            retention_days: 45,
        },
    ];

    let rendered = render_mise_task_suites(&spec(vec![push]))?;
    let yaml = &rendered[0].bytes;
    assert_eq!(yaml.matches(crate::steps::UPLOAD_ARTIFACT_USES).count(), 2);
    assert_eq!(yaml.matches("if-no-files-found: error").count(), 2);
    assert!(yaml.contains("path: target/evidence/manifest.json"));
    assert!(yaml.contains("path: target/evidence/push-event.json"));
    assert!(
        yaml.contains("artifact-8-manifest-${{ github.run_id }}-attempt-${{ github.run_attempt }}")
    );
    assert!(
        yaml.contains(
            "artifact-10-push-event-${{ github.run_id }}-attempt-${{ github.run_attempt }}"
        )
    );
    Ok(())
}

#[test]
fn artifact_identity_separates_hyphenated_suite_and_artifact_ids() -> TestResult {
    let mut suite_a = policy("a", vec![MiseTaskSuiteEvent::Push], Vec::new());
    suite_a.suite.artifacts.push(RustSuiteArtifact {
        name: "b-c".to_owned(),
        paths: vec!["target/evidence/first.json".to_owned()],
        when: RustSuiteArtifactWhen::Success,
        retention_days: 45,
    });
    let mut suite_ab = policy("a-b", vec![MiseTaskSuiteEvent::Push], Vec::new());
    suite_ab.suite.artifacts.push(RustSuiteArtifact {
        name: "c".to_owned(),
        paths: vec!["target/evidence/second.json".to_owned()],
        when: RustSuiteArtifactWhen::Success,
        retention_days: 45,
    });

    let rendered = render_mise_task_suites(&spec(vec![suite_a, suite_ab]))?;
    let yaml = &rendered[0].bytes;
    let first = "velnor-mise-task-suite-1-a-artifact-3-b-c-${{ github.run_id }}-attempt-${{ github.run_attempt }}";
    let second = "velnor-mise-task-suite-3-a-b-artifact-1-c-${{ github.run_id }}-attempt-${{ github.run_attempt }}";
    assert_ne!(first, second);
    assert!(yaml.contains(first), "{yaml}");
    assert!(yaml.contains(second), "{yaml}");
    Ok(())
}

#[test]
fn suite_tool_bootstrap_rejects_missing_lock_for_selected_tools() {
    let mut suite = policy("compile", vec![MiseTaskSuiteEvent::Push], Vec::new());
    suite.selected_tools.push(BuildTaskTool {
        key: "rust".to_owned(),
        version: "1.99.1".to_owned(),
        backend: "core:rust".to_owned(),
        os: Vec::new(),
        config_options: BTreeMap::new(),
        lock_options: BTreeMap::new(),
        artifact: None,
    });
    assert!(render_mise_task_suites(&spec(vec![suite])).is_err());
}

#[path = "mise_task_suites_evidence_tests.rs"]
mod evidence_tests;
