//! Isolated Cargo source production prepares qualified tools without publication.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CacheMode, CacheSnapshotDomain, CompiledSourceHelper, Job, JobTimeout, PermissionLevel,
    Permissions, SourceProducer, SourceProducerRole, Step, StepId, ToolCacheDescriptor,
    ToolCacheDomain, ToolProducerSelection,
};
use velnor_actions_mise::catalog::qualification::DistributionHost;
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::MiseSetup;

use super::{descriptor::RustSourceDescriptor, transport::Source3};
use crate::{
    OrchestratorError,
    workloads::cache::{source_report, source_snapshot},
};

const TOOLS_ID: &str = "velnor-rust-source-tools";
const BEFORE_ID: &str = "velnor-rust-source-before";
const AFTER_ID: &str = "velnor-rust-source-after";
const VERIFY_ID: &str = "velnor-rust-source-verify";

/// Build a source writer with read-only tools and a mandatory publication report.
pub(crate) fn producer_job(
    descriptor: &RustSourceDescriptor,
    catalog: &ToolCatalog,
    label: &str,
    setup: &MiseSetup,
    version: &str,
    selection: &ToolProducerSelection,
) -> Result<(Job, Vec<CompiledSourceHelper>), OrchestratorError> {
    let (tool, installation) = tool_preparation(catalog, label, setup, version)?;
    let base = super::source::compiled_helper(descriptor, version)?;
    let source = Source3::new(super::descriptor::source_identity(descriptor, &base)?)?;
    let metadata = source_metadata(&source, &tool, selection)?;
    let helper = base.with_environment(helper_environment(&source));
    let mut steps =
        velnor_actions_workflow_renderer::tool_producer_steps::tool_consumer_steps(&tool, setup)?;
    steps.extend([
        preparation_step(&installation)?,
        observer_step(CacheSnapshotDomain::Tools, true, TOOLS_ID, version)?,
        source.restore()?,
        observer_step(CacheSnapshotDomain::Sources, true, BEFORE_ID, version)?,
        verification_step(&helper)?,
        observer_step(CacheSnapshotDomain::Sources, false, AFTER_ID, version)?,
        source.save(&metadata)?,
        source.publication(&metadata)?,
        source_report::step(&metadata, version)?,
    ]);
    let records = vec![
        helper,
        installation,
        setup
            .bootstrap(ToolCacheDomain::Full, label)?
            .helper
            .clone(),
        source_snapshot::record(CacheSnapshotDomain::Tools, true, version)?,
        source_snapshot::record(CacheSnapshotDomain::Sources, true, version)?,
        source_snapshot::record(CacheSnapshotDomain::Sources, false, version)?,
        source_report::record(&metadata, version)?,
    ];
    Ok((
        Job {
            display_name: "Public Cargo sources".to_owned(),
            runs_on: label.to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: selection.needs(ToolCacheDomain::Full),
            condition: Some(selection.condition(ToolCacheDomain::Full)),
            cache_mode: Some(CacheMode::Write),
            permissions: Some(empty_permissions()),
            environment: None,
            tool_producer: None,
            mbx_producer: None,
            outputs: source_report::outputs(&metadata),
            source_producer: Some(metadata),
            native_pages_deploy: None,
            native_publish: None,
            steps,
        },
        records,
    ))
}

fn tool_preparation(
    catalog: &ToolCatalog,
    label: &str,
    setup: &MiseSetup,
    version: &str,
) -> Result<(ToolCacheDescriptor, CompiledSourceHelper), OrchestratorError> {
    let target = velnor_actions_contract::tool_target_for_runner_label(label)
        .ok_or_else(|| contract_error("rust_source_unsupported_runner"))?;
    if target != DistributionHost::LinuxAmd64.abi() {
        return Err(contract_error("rust_source_unsupported_runner"));
    }
    let installation = velnor_actions_mise::catalog::tool_prepare::helper_for_tools(
        catalog,
        ToolCacheDomain::Full,
        DistributionHost::LinuxAmd64,
        &[catalog.tool_spec(PinnedTool::Rust)?],
        version,
    )
    .map_err(contract_error)?;
    let descriptor = velnor_actions_workflow_renderer::tool_producer_steps::descriptor_for_record(
        &installation,
        label,
        target,
        ToolCacheDomain::Full,
        setup,
        std::slice::from_ref(&installation),
    )?;
    Ok((descriptor, installation))
}

fn source_metadata(
    source: &Source3,
    tool: &ToolCacheDescriptor,
    selection: &ToolProducerSelection,
) -> Result<SourceProducer, OrchestratorError> {
    let metadata = SourceProducer {
        role: SourceProducerRole::Cargo,
        selection: selection.clone(),
        tool_cache: Some(tool.clone()),
        source_identity: source.identity().to_owned(),
        verification_step: StepId::new(VERIFY_ID)?,
        restore_step: StepId::new("velnor-sources-cache")?,
        save_step: StepId::new("velnor-rust-source-save")?,
        publication_step: StepId::new("velnor-rust-source-publication")?,
        report_step: StepId::new(source_report::REPORT_ID)?,
    };
    metadata.validate()?;
    Ok(metadata)
}

pub(super) fn helper_environment(source: &Source3) -> BTreeMap<String, String> {
    BTreeMap::from([(
        "VELNOR_SOURCE_IDENTITY".to_owned(),
        source.identity().to_owned(),
    )])
}

fn preparation_step(helper: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Prepare source Rust toolchain",
        helper,
        helper.environment().clone(),
    )?;
    step.id = Some(StepId::new("velnor-rust-source-prepare")?);
    Ok(step)
}

fn observer_step(
    layer: CacheSnapshotDomain,
    before: bool,
    id: &str,
    version: &str,
) -> Result<Step, OrchestratorError> {
    let mut step = source_snapshot::step(layer, before, version)?;
    step.id = Some(StepId::new(id)?);
    Ok(step)
}

fn verification_step(helper: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Verify public Cargo sources",
        helper,
        helper.environment().clone(),
    )?;
    step.id = Some(StepId::new(VERIFY_ID)?);
    Ok(step)
}

fn empty_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::None,
        actions: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        issues: PermissionLevel::None,
        pages: PermissionLevel::None,
        attestations: PermissionLevel::None,
    }
}

fn contract_error(error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}
