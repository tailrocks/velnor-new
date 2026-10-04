use super::{RUNNER, build_job, selection, setup, sources};
use velnor_actions_contract::workflow::{JobOutput, StepOutputRef};
use velnor_actions_contract::{
    ActionOutput, CacheMode, SourceBoundOperation, StepKind, ToolCacheDomain,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[test]
fn producer_uses_central_cache_gate_and_typed_publication_outputs() {
    let catalog = ToolCatalog::pinned();
    let selected = selection();
    let job = build_job(&sources(), &catalog, &selected).expect("producer job");
    let metadata = job.source_producer.as_ref().expect("source metadata");
    assert_eq!(
        (
            metadata.verification_step.as_str(),
            metadata.restore_step.as_str(),
            metadata.save_step.as_str(),
            metadata.publication_step.as_str(),
            metadata.report_step.as_str(),
        ),
        (
            "velnor-npm-public-proof",
            "velnor-npm-cache",
            "velnor-npm-source-save",
            "velnor-npm-source-publication",
            "velnor-source-report",
        )
    );
    let condition = metadata.condition();
    assert_eq!(job.condition.as_deref(), Some(condition.as_str()));
    assert_eq!(job.cache_mode, Some(CacheMode::Write));

    let expected_outputs = [
        ActionOutput::CacheAvailable,
        ActionOutput::Verified,
        ActionOutput::SourceIdentity,
        ActionOutput::Error,
    ]
    .into_iter()
    .map(|output| JobOutput {
        name: output.as_str().to_owned(),
        value: StepOutputRef {
            step_id: metadata.report_step.clone(),
            output,
        },
    })
    .collect::<Vec<_>>();
    assert_eq!(job.outputs, expected_outputs);
}

#[test]
fn source_role_binds_descriptor_and_canonical_bootstrap_prefix() {
    let catalog = ToolCatalog::pinned();
    let selected = selection();
    let job = build_job(&sources(), &catalog, &selected).expect("producer job");
    let metadata = job.source_producer.as_ref().expect("source metadata");
    let Some(descriptor) = metadata.tool_cache.as_ref() else {
        panic!("npm bootstrap descriptor");
    };
    assert_eq!(descriptor.domain, ToolCacheDomain::NpmBootstrap);
    assert_eq!(descriptor.runs_on, RUNNER);
    assert_eq!(
        descriptor.target,
        velnor_actions_contract::tool_target_for_runner_label(RUNNER).expect("runner target")
    );
    assert_eq!(
        descriptor.selectors,
        catalog
            .native_tool_specs(
                velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64,
                &[PinnedTool::Node],
            )
            .expect("qualified selectors")
    );
    assert!(
        descriptor
            .qualification_identity
            .starts_with("qualified-tools@")
    );
    let prefix = velnor_actions_workflow_renderer::tool_producer_steps::tool_consumer_steps(
        descriptor,
        &setup(),
    )
    .expect("canonical bootstrap prefix");
    assert_eq!(&job.steps[..prefix.len()], prefix.as_slice());
    let prepare_at = job
        .steps
        .iter()
        .position(|step| {
            step.id
                .as_ref()
                .is_some_and(|id| id.as_str() == "velnor-npm-source-prepare")
        })
        .expect("canonical prepare step");
    assert_eq!(prepare_at, prefix.len());
    assert_eq!(
        job.steps
            .iter()
            .filter(|step| matches!(
                &step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == SourceBoundOperation::MiseToolPrepare
            ))
            .count(),
        1,
        "one canonical tool installation"
    );
    let StepKind::SourceBoundHelper { invocation, .. } = &job.steps[prepare_at].kind else {
        panic!("canonical prepare helper");
    };
    assert_eq!(
        invocation.descriptor().operation(),
        SourceBoundOperation::MiseToolPrepare
    );
}

#[test]
fn producer_publication_receipt_reuses_canonical_save() {
    let catalog = ToolCatalog::pinned();
    let selected = selection();
    let job = build_job(&sources(), &catalog, &selected).expect("producer job");
    let metadata = job.source_producer.as_ref().expect("source metadata");
    let save_at = job
        .steps
        .iter()
        .position(|step| step.id.as_ref() == Some(&metadata.save_step))
        .expect("save step");
    let receipt_at = job
        .steps
        .iter()
        .position(|step| step.id.as_ref() == Some(&metadata.publication_step))
        .expect("publication receipt");
    assert_eq!(save_at, job.steps.len() - 3);
    assert_eq!(receipt_at, job.steps.len() - 2);
    let StepKind::Action {
        with: save_with, ..
    } = &job.steps[save_at].kind
    else {
        panic!("source save action");
    };
    assert_eq!(save_with["key"], metadata.save_key());
    let save_condition = metadata.save_condition();
    assert_eq!(
        job.steps[save_at].condition.as_deref(),
        Some(save_condition.as_str())
    );
    let StepKind::Action { uses, with, .. } = &job.steps[receipt_at].kind else {
        panic!("publication receipt action");
    };
    assert_eq!(
        uses,
        velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES
    );
    assert_eq!(with["key"], metadata.save_key());
    assert_eq!(with["lookup-only"], "true");
    assert_eq!(
        with["path"],
        super::super::super::npm::payload_paths().join("\n")
    );
    let receipt_condition = metadata.publication_condition();
    assert_eq!(
        job.steps[receipt_at].condition.as_deref(),
        Some(receipt_condition.as_str())
    );
    assert_eq!(
        job.steps.last().and_then(|step| step.id.as_ref()),
        Some(&metadata.report_step)
    );
}
