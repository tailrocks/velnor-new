#[path = "cache_source_roles_fixture_records.rs"]
mod records;
use records::{compiled_record, owner_record};

use super::{source_outputs, validate_source_producer};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CacheMode, Job, JobTimeout, PermissionLevel, Permissions, SourceBoundOperation, SourceProducer,
    SourceProducerRole, Step, StepId, StepKind, ToolCacheDomain, ToolProducerSelection,
};

const RUNNER: &str = "ubuntu-24.04";
const TARGET: &str = "x86_64-unknown-linux-gnu";
const SOURCE_IDENTITY: &str = "npm-source-test-v1";
const NPM_PATHS: &str = "${{ runner.temp }}/velnor/native/npm/_cacache/content-v2\n${{ runner.temp }}/velnor/native/npm/public-proof-v1.json";
const PREPARE_ID: &str = "npm-source-prepare";
const PROOF_ID: &str = "npm-public-proof";
const RESTORE_ID: &str = "npm-source-restore";
const SAVE_ID: &str = "npm-source-save";
const PUBLICATION_ID: &str = "npm-source-publication";
const REPORT_ID: &str = "velnor-source-report";
const BEFORE_ID: &str = "velnor-npm-source-before";
const AFTER_ID: &str = "velnor-npm-source-after";

pub(super) fn setup() -> crate::MiseSetup {
    crate::setup::fixture::mise_setup("2026.9.16", &"a".repeat(64))
}

fn id(value: &str) -> StepId {
    StepId::new(value).expect("step id")
}

fn selection() -> ToolProducerSelection {
    ToolProducerSelection {
        unconditional: true,
        ..ToolProducerSelection::default()
    }
}

pub(super) fn fixture_records() -> Vec<velnor_actions_contract::CompiledSourceHelper> {
    vec![owner_record(
        SourceBoundOperation::MiseToolPrepare,
        &["node@24.20.0"],
    )]
}

fn descriptor() -> velnor_actions_contract::ToolCacheDescriptor {
    crate::tool_producer_steps::descriptor_for_record(
        &owner_record(SourceBoundOperation::MiseToolPrepare, &["node@24.20.0"]),
        RUNNER,
        TARGET,
        ToolCacheDomain::NpmBootstrap,
        &setup(),
        &fixture_records(),
    )
    .expect("tool descriptor")
}

fn metadata() -> SourceProducer {
    SourceProducer {
        role: SourceProducerRole::Npm,
        selection: selection(),
        tool_cache: Some(descriptor()),
        source_identity: SOURCE_IDENTITY.to_owned(),
        verification_step: id(PROOF_ID),
        restore_step: id(RESTORE_ID),
        save_step: id(SAVE_ID),
        publication_step: id(PUBLICATION_ID),
        report_step: id(REPORT_ID),
    }
}

fn permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
        issues: PermissionLevel::None,
        attestations: PermissionLevel::None,
        pages: PermissionLevel::None,
    }
}

fn source_helper(
    name: &str,
    operation: SourceBoundOperation,
    selectors: &[&str],
    step_id: &str,
) -> Step {
    let record = owner_record(operation, selectors);
    let mut step =
        crate::source_helper::source_helper_step(name, &record, record.environment().clone())
            .expect("source helper step");
    step.id = Some(id(step_id));
    step
}

fn source_restore() -> Step {
    let mut step = crate::action_step(
        "Restore npm source",
        crate::steps::TOOLS_RESTORE_USES,
        BTreeMap::from([
            ("path".to_owned(), NPM_PATHS.to_owned()),
            ("key".to_owned(), format!("{SOURCE_IDENTITY}-lookup")),
            (
                "restore-keys".to_owned(),
                format!("{SOURCE_IDENTITY}-snapshot-"),
            ),
        ]),
    )
    .expect("source restore");
    step.id = Some(id(RESTORE_ID));
    step
}

fn snapshot_step(before: bool) -> Step {
    let phase = if before { "before" } else { "after" };
    let env = velnor_actions_contract::CacheSnapshotDomain::NpmDownloads.environment(before);
    let record = compiled_record(SourceBoundOperation::CacheSnapshot, Vec::new(), env);
    let mut step = crate::source_helper::source_helper_step(
        "Measure npm source snapshot",
        &record,
        record.environment().clone(),
    )
    .expect("snapshot helper");
    step.id = Some(id(if before { BEFORE_ID } else { AFTER_ID }));
    if !before {
        step.condition = Some(format!("steps.{PROOF_ID}.outputs.verified == 'true'"));
    }
    assert_eq!(
        record.invocation().args(),
        ["npm_downloads".to_owned(), phase.to_owned()]
    );
    step
}

fn source_save(meta: &SourceProducer) -> Step {
    let mut step = crate::action_step(
        "Save npm source",
        crate::steps::TOOLS_SAVE_USES,
        BTreeMap::from([
            ("path".to_owned(), NPM_PATHS.to_owned()),
            ("key".to_owned(), meta.save_key()),
        ]),
    )
    .expect("source save");
    step.id = Some(id(SAVE_ID));
    step.condition = Some(meta.save_condition());
    step
}

fn source_publication(meta: &SourceProducer) -> Step {
    let mut step = crate::action_step(
        "Verify npm source publication",
        crate::steps::TOOLS_RESTORE_USES,
        BTreeMap::from([
            ("path".to_owned(), NPM_PATHS.to_owned()),
            ("key".to_owned(), meta.save_key()),
            ("lookup-only".to_owned(), "true".to_owned()),
        ]),
    )
    .expect("publication lookup");
    step.id = Some(id(PUBLICATION_ID));
    step.condition = Some(meta.publication_condition());
    step
}

fn report_environment(meta: &SourceProducer) -> BTreeMap<String, String> {
    let outcome = |step: &StepId| format!("${{{{ steps.{}.outcome }}}}", step.as_str());
    BTreeMap::from([
        (
            "VELNOR_SOURCE_IDENTITY".to_owned(),
            meta.source_identity.clone(),
        ),
        (
            "VELNOR_SOURCE_OUTCOME".to_owned(),
            outcome(&meta.verification_step),
        ),
        (
            "VELNOR_SOURCE_VERIFIED".to_owned(),
            format!("${{{{ steps.{PROOF_ID}.outputs.verified }}}}"),
        ),
        (
            "VELNOR_SOURCE_ERROR".to_owned(),
            format!("${{{{ steps.{PROOF_ID}.outputs.error }}}}"),
        ),
        (
            "VELNOR_SOURCE_RESTORE_KEY".to_owned(),
            format!("${{{{ steps.{RESTORE_ID}.outputs.cache-matched-key }}}}"),
        ),
        (
            "VELNOR_SOURCE_SAVE_OUTCOME".to_owned(),
            outcome(&meta.save_step),
        ),
        (
            "VELNOR_SOURCE_RESTORE_OUTCOME".to_owned(),
            outcome(&meta.restore_step),
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_OUTCOME".to_owned(),
            outcome(&meta.publication_step),
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_MATCHED_KEY".to_owned(),
            format!("${{{{ steps.{PUBLICATION_ID}.outputs.cache-matched-key }}}}"),
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY".to_owned(),
            meta.save_key(),
        ),
        (
            "VELNOR_SOURCE_SNAPSHOT_OUTCOME".to_owned(),
            "${{ steps.velnor-npm-source-after.outcome }}".to_owned(),
        ),
        (
            "VELNOR_SOURCE_SNAPSHOT_CHANGED".to_owned(),
            "${{ env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_CHANGED }}".to_owned(),
        ),
    ])
}

fn source_report(meta: &SourceProducer) -> Step {
    let record = compiled_record(
        SourceBoundOperation::SourceProducerReport,
        Vec::new(),
        report_environment(meta),
    );
    let mut step = crate::source_helper::source_helper_step(
        "Report source availability",
        &record,
        record.environment().clone(),
    )
    .expect("source report");
    step.id = Some(id(REPORT_ID));
    step.condition = Some("always()".to_owned());
    step
}

pub(super) fn producer_job() -> Job {
    let meta = metadata();
    let descriptor = meta.tool_cache.as_ref().expect("descriptor");
    let mut steps = crate::tool_producer_steps::tool_consumer_steps(descriptor, &setup())
        .expect("consumer prefix");
    steps.extend([
        source_helper(
            "Prepare isolated npm source tools",
            SourceBoundOperation::MiseToolPrepare,
            &["node@24.20.0"],
            PREPARE_ID,
        ),
        source_restore(),
        snapshot_step(true),
        source_helper(
            "Prove npm public source",
            SourceBoundOperation::NpmPublicSourceProducer,
            &[],
            PROOF_ID,
        ),
        snapshot_step(false),
        source_save(&meta),
        source_publication(&meta),
        source_report(&meta),
    ]);
    Job {
        cache_mode: Some(CacheMode::Write),
        display_name: "Public npm sources".to_owned(),
        runs_on: RUNNER.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: meta.selection.needs(descriptor.domain),
        condition: Some(meta.condition()),
        permissions: Some(permissions()),
        environment: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: Some(meta.clone()),
        native_pages_deploy: None,
        native_publish: None,
        outputs: source_outputs(&meta),
        steps,
    }
}

fn helper_environments(job: &Job) -> Vec<BTreeMap<String, String>> {
    job.steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::SourceBoundHelper { env, .. } => Some(env.clone()),
            _ => None,
        })
        .collect()
}

fn assert_rejected(job: Job, reason: &str) {
    let error = validate_source_producer(&job, &setup(), &fixture_records())
        .expect_err("source producer rejected");
    assert!(error.to_string().contains(reason), "{error}");
}

#[test]
fn source_producer_membership_is_closed_to_native_operations() {
    for operation in [
        SourceBoundOperation::NpmPublicSourceProducer,
        SourceBoundOperation::BunSourceProducer,
        SourceBoundOperation::GradleSourceProducer,
        SourceBoundOperation::RustSourceProducer,
        SourceBoundOperation::TofuProviderExport,
        SourceBoundOperation::TofuRootOwnership,
        SourceBoundOperation::SourceProducerReport,
    ] {
        assert!(operation.is_source_producer(), "{operation:?}");
    }
    for operation in [
        SourceBoundOperation::CacheSnapshot,
        SourceBoundOperation::RustPrepareRootLinux,
        SourceBoundOperation::RustPrepareDesktopMac,
        SourceBoundOperation::RustPrepareDesktopSourceMac,
        SourceBoundOperation::DesktopNativeHydration,
        SourceBoundOperation::HomebrewPreparation,
    ] {
        assert!(!operation.is_source_producer(), "{operation:?}");
    }
}

#[test]
fn pure_npm_producer_preserves_owned_setup_helpers_and_writer_policy() {
    let mut job = producer_job();
    let before_steps = job.steps.clone();
    let before_environments = helper_environments(&job);
    assert!(job.source_producer.is_some());
    assert!(validate_source_producer(&job, &setup(), &fixture_records()).expect("source producer"));
    assert_eq!(
        crate::cache_p08::infer_job_tools(&job),
        vec!["node@24.20.0"]
    );
    crate::cache_p08::ensure_setup_p08(
        "source",
        &mut job,
        &setup(),
        true,
        TARGET,
        &fixture_records(),
    )
    .expect("source setup remains canonical");
    assert_eq!(job.steps, before_steps);
    assert_eq!(helper_environments(&job), before_environments);
    let jobs = BTreeMap::from([("source".to_owned(), job)]);
    crate::cache_p08::validate_tool_consumers(&jobs, &setup(), &fixture_records())
        .expect("writer policy");
    assert_eq!(jobs["source"].steps, before_steps);
}

#[path = "cache_source_roles_save_tests.rs"]
mod save;

#[path = "cache_source_roles_rejection_tests.rs"]
mod rejection;
