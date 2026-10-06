use super::{RUNNER, setup};
use crate::workloads::cache_eligibility::NativeNpmSource;
use velnor_actions_contract::{Job, SourceBoundOperation, StepKind, ToolProducerSelection};
use velnor_actions_mise::ToolCatalog;

fn runtime_mismatch(error: crate::OrchestratorError) -> bool {
    error
        .to_string()
        .contains("npm_source_runtime_version_mismatch")
}

pub(super) fn assert_authority_records(
    job: &Job,
    candidates: &[NativeNpmSource],
    catalog: &ToolCatalog,
    selection: &ToolProducerSelection,
) {
    assert_runtime_version(candidates, catalog, selection);
    let mut records = super::super::source_records(
        candidates,
        catalog,
        RUNNER,
        &setup(),
        env!("CARGO_PKG_VERSION"),
        selection,
    )
    .expect("source authority records");
    records.push(
        setup()
            .bootstrap(
                velnor_actions_contract::ToolCacheDomain::NpmBootstrap,
                RUNNER,
            )
            .expect("bootstrap authority")
            .helper
            .clone(),
    );
    velnor_actions_workflow_renderer::source_helper::validate_registry(
        &records,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("registered source records");
    assert!(records.iter().any(|record| {
        record.invocation().descriptor().operation() == SourceBoundOperation::MiseToolPrepare
    }));
    let helpers = job.steps.iter().filter_map(|step| match &step.kind {
        StepKind::SourceBoundHelper { invocation, env } => Some((invocation, env)),
        _ => None,
    });
    assert!(helpers.into_iter().all(|(invocation, env)| {
        records
            .iter()
            .any(|record| record.invocation() == invocation && record.environment() == env)
    }));
    let snapshots = records
        .iter()
        .filter(|record| {
            record.invocation().descriptor().operation() == SourceBoundOperation::CacheSnapshot
        })
        .collect::<Vec<_>>();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].source(), snapshots[1].source());
    assert_eq!(
        snapshots[0].invocation().descriptor(),
        snapshots[1].invocation().descriptor()
    );
    assert_ne!(
        snapshots[0].invocation().args(),
        snapshots[1].invocation().args()
    );
}

fn assert_runtime_version(
    candidates: &[NativeNpmSource],
    catalog: &ToolCatalog,
    selection: &ToolProducerSelection,
) {
    let unsupported_version = "0.0.0";
    assert!(
        super::super::producer_job(
            candidates,
            catalog,
            RUNNER,
            &setup(),
            unsupported_version,
            selection,
        )
        .is_err_and(runtime_mismatch)
    );
    assert!(
        super::super::source_records(
            candidates,
            catalog,
            RUNNER,
            &setup(),
            unsupported_version,
            selection,
        )
        .is_err_and(runtime_mismatch)
    );
}
