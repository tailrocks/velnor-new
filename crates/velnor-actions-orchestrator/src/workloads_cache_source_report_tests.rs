use super::{depend_on_producer, outputs, publication_step};
use std::{fs, process::Command};
use velnor_actions_contract::{
    ActionOutput, Job, JobTimeout, SourceProducer, SourceProducerRole, StepId, StepKind,
};

const IDENTITY: &str = "source-key";
const REPORT: &str = include_str!("workloads_cache_source_report.sh");

fn run_report(overrides: &[(&str, &str)]) -> String {
    let temp = tempfile::tempdir().expect("output tempdir");
    let output = temp.path().join("github-output");
    let mut command = Command::new("/bin/bash");
    command
        .args(["--noprofile", "--norc", "-p", "-c", REPORT, "source-report"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GITHUB_OUTPUT", &output)
        .env("VELNOR_SOURCE_IDENTITY", IDENTITY)
        .env("VELNOR_SOURCE_OUTCOME", "skipped")
        .env("VELNOR_SOURCE_VERIFIED", "false")
        .env("VELNOR_SOURCE_ERROR", "NONE")
        .env("VELNOR_SOURCE_SAVE_OUTCOME", "skipped")
        .env("VELNOR_SOURCE_RESTORE_OUTCOME", "skipped")
        .env("VELNOR_SOURCE_RESTORE_KEY", "")
        .env("VELNOR_SOURCE_SNAPSHOT_OUTCOME", "success")
        .env("VELNOR_SOURCE_SNAPSHOT_CHANGED", "false")
        .env("VELNOR_SOURCE_PUBLICATION_OUTCOME", "skipped")
        .env("VELNOR_SOURCE_PUBLICATION_MATCHED_KEY", "")
        .env(
            "VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY",
            "source-key-publication",
        );
    for (name, value) in overrides {
        command.env(name, value);
    }
    let status = command.status().expect("source report shell");
    assert!(status.success(), "source report failed: {status}");
    fs::read_to_string(output).expect("source report output")
}

fn expected(cache_available: bool, verified: bool, error: &str) -> String {
    format!(
        "cache_available={cache_available}\nverified={verified}\nsourceidentity={IDENTITY}\nerror={error}\n"
    )
}

fn metadata() -> SourceProducer {
    metadata_with_role(SourceProducerRole::Npm)
}

fn metadata_with_role(role: SourceProducerRole) -> SourceProducer {
    SourceProducer {
        role,
        selection: Default::default(),
        tool_cache: None,
        source_identity: IDENTITY.to_owned(),
        verification_step: StepId::new("verify").expect("verification ID"),
        restore_step: StepId::new("restore").expect("restore ID"),
        save_step: StepId::new("save").expect("save ID"),
        publication_step: StepId::new("publication").expect("publication ID"),
        report_step: StepId::new("report").expect("report ID"),
    }
}

fn consumer(needs: Vec<String>, condition: &str) -> Job {
    Job {
        cache_mode: None,
        display_name: "consumer".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs,
        condition: Some(condition.to_owned()),
        permissions: None,
        environment: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps: Vec::new(),
    }
}

#[test]
fn skipped_preparation_reports_preparation_failed() {
    assert_eq!(
        run_report(&[]),
        expected(false, false, "PREPARATION_FAILED")
    );
}

#[test]
fn failed_source_reports_source_verification_failed() {
    assert_eq!(
        run_report(&[("VELNOR_SOURCE_OUTCOME", "failure")]),
        expected(false, false, "SOURCE_VERIFICATION_FAILED")
    );
}

#[test]
fn verified_source_with_failed_save_reports_transport_failure() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_SAVE_OUTCOME", "failure"),
        ]),
        expected(false, true, "CACHE_TRANSPORT_FAILED")
    );
}

#[test]
fn successful_save_requires_a_matching_publication_receipt() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_SAVE_OUTCOME", "success"),
            ("VELNOR_SOURCE_PUBLICATION_OUTCOME", "success"),
            (
                "VELNOR_SOURCE_PUBLICATION_MATCHED_KEY",
                "source-key-publication",
            ),
        ]),
        expected(true, true, "NONE")
    );
    for outcome in ["skipped", ""] {
        assert_eq!(
            run_report(&[
                ("VELNOR_SOURCE_OUTCOME", "success"),
                ("VELNOR_SOURCE_VERIFIED", "true"),
                ("VELNOR_SOURCE_SAVE_OUTCOME", "success"),
                ("VELNOR_SOURCE_PUBLICATION_OUTCOME", "success"),
                (
                    "VELNOR_SOURCE_PUBLICATION_MATCHED_KEY",
                    "source-key-publication"
                ),
                ("VELNOR_SOURCE_SNAPSHOT_OUTCOME", outcome),
            ]),
            expected(false, false, "SOURCE_VERIFICATION_FAILED")
        );
    }
}

#[test]
fn successful_save_without_publication_receipt_is_transport_unavailable() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_SAVE_OUTCOME", "success"),
            ("VELNOR_SOURCE_PUBLICATION_OUTCOME", "success"),
        ]),
        expected(false, true, "CACHE_TRANSPORT_UNAVAILABLE")
    );
}

#[test]
fn failed_publication_lookup_is_transport_unavailable() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_SAVE_OUTCOME", "success"),
            ("VELNOR_SOURCE_PUBLICATION_OUTCOME", "failure"),
        ]),
        expected(false, true, "CACHE_TRANSPORT_UNAVAILABLE")
    );
}

#[test]
fn mismatched_publication_receipt_is_not_published() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_SAVE_OUTCOME", "success"),
            ("VELNOR_SOURCE_PUBLICATION_OUTCOME", "success"),
            (
                "VELNOR_SOURCE_PUBLICATION_MATCHED_KEY",
                "other-source-publication",
            ),
        ]),
        expected(false, true, "CACHE_NOT_PUBLISHED")
    );
}

#[test]
fn exact_current_key_restore_proves_availability_when_save_is_skipped() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_RESTORE_OUTCOME", "success"),
            ("VELNOR_SOURCE_RESTORE_KEY", IDENTITY),
        ]),
        expected(true, true, "NONE")
    );
}

#[test]
fn current_identity_snapshot_key_proves_availability_when_save_is_skipped() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_RESTORE_OUTCOME", "success"),
            ("VELNOR_SOURCE_RESTORE_KEY", "source-key-snapshot-previous"),
        ]),
        expected(true, true, "NONE")
    );
}

#[test]
fn changed_snapshot_does_not_prove_repaired_current_identity() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_RESTORE_OUTCOME", "success"),
            ("VELNOR_SOURCE_RESTORE_KEY", IDENTITY),
            ("VELNOR_SOURCE_SNAPSHOT_CHANGED", "true"),
        ]),
        expected(false, true, "CACHE_NOT_PUBLISHED")
    );
}

#[test]
fn failed_after_snapshot_does_not_prove_current_publication() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_RESTORE_OUTCOME", "success"),
            ("VELNOR_SOURCE_RESTORE_KEY", IDENTITY),
            ("VELNOR_SOURCE_SNAPSHOT_OUTCOME", "failure"),
        ]),
        expected(false, false, "SOURCE_VERIFICATION_FAILED")
    );
}

#[test]
fn skipped_save_with_failed_restore_is_transport_unavailable() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_RESTORE_OUTCOME", "failure"),
        ]),
        expected(false, true, "CACHE_TRANSPORT_UNAVAILABLE")
    );
}

#[test]
fn compatible_fallback_key_does_not_prove_current_publication() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_RESTORE_OUTCOME", "success"),
            (
                "VELNOR_SOURCE_RESTORE_KEY",
                "compatible-key-snapshot-previous"
            ),
        ]),
        expected(false, true, "CACHE_NOT_PUBLISHED")
    );
}

#[test]
fn private_proof_decision_cannot_be_faked_by_success_fields() {
    assert_eq!(
        run_report(&[
            ("VELNOR_SOURCE_OUTCOME", "success"),
            ("VELNOR_SOURCE_VERIFIED", "true"),
            ("VELNOR_SOURCE_ERROR", "PRIVATE_OR_AUTH_REQUIRED"),
            ("VELNOR_SOURCE_SAVE_OUTCOME", "success"),
        ]),
        expected(false, false, "PRIVATE_OR_AUTH_REQUIRED")
    );
}

#[test]
fn publication_step_is_exact_lookup_only_transport() {
    let meta = metadata();
    let paths = vec![
        "${{ runner.temp }}/payload/a".to_owned(),
        "payload/b".to_owned(),
    ];
    let step = publication_step(&meta, &paths).expect("publication lookup");
    assert_eq!(step.id, Some(meta.publication_step.clone()));
    assert_eq!(step.condition, Some(meta.publication_condition()));
    let StepKind::Action { uses, with, .. } = step.kind else {
        panic!("publication lookup action");
    };
    assert_eq!(
        uses,
        velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES
    );
    assert_eq!(with["key"], meta.save_key());
    assert_eq!(with["path"], paths.join("\n"));
    assert_eq!(with["lookup-only"], "true");
    assert!(!with.contains_key("restore-keys"));
}

#[test]
fn outputs_bind_all_terminal_report_fields() {
    let meta = metadata();
    let outputs = outputs(&meta);
    assert_eq!(outputs.len(), 4);
    assert_eq!(
        outputs
            .iter()
            .map(|output| (output.name.as_str(), output.value.output))
            .collect::<Vec<_>>(),
        vec![
            ("cache_available", ActionOutput::CacheAvailable),
            ("verified", ActionOutput::Verified),
            ("sourceidentity", ActionOutput::SourceIdentity),
            ("error", ActionOutput::Error),
        ]
    );
    assert!(
        outputs
            .iter()
            .all(|output| output.value.step_id == meta.report_step)
    );
}

#[test]
fn depend_on_producer_preserves_guard_and_requires_successful_plan() {
    let mut consumer = consumer(
        vec!["plan".to_owned()],
        "${{ !cancelled() && github.ref == 'refs/heads/main' }}",
    );
    depend_on_producer(&mut consumer, "source-producer");
    assert_eq!(
        consumer.needs,
        vec!["plan".to_owned(), "source-producer".to_owned()]
    );
    assert_eq!(
        consumer.condition.as_deref(),
        Some(
            "!cancelled() && needs.plan.result == 'success' && (\
!cancelled() && github.ref == 'refs/heads/main')"
        )
    );
}

#[test]
fn depend_on_producer_expands_success_before_adding_new_dependency() {
    let mut consumer = consumer(
        vec!["plan".to_owned(), "build".to_owned()],
        "${{ success() && (needs.plan.outputs.covered == 'true' || \
needs.build.outputs.covered == 'true') }}",
    );
    depend_on_producer(&mut consumer, "source-producer");
    assert_eq!(
        consumer.needs,
        vec![
            "plan".to_owned(),
            "build".to_owned(),
            "source-producer".to_owned()
        ]
    );
    let condition = consumer.condition.expect("wrapped condition");
    assert!(condition.contains("needs['plan'].result == 'success'"));
    assert!(condition.contains("needs['build'].result == 'success'"));
    assert!(condition.contains("needs.plan.outputs.covered == 'true'"));
    assert!(condition.contains("needs.build.outputs.covered == 'true'"));
    assert!(!condition.contains("success()"));
    assert!(!condition.contains("needs['source-producer'].result"));
}

#[path = "workloads_cache_source_report_factory_tests.rs"]
mod factory_tests;

#[path = "workloads_cache_source_report_receipt_tests.rs"]
mod receipt_tests;
