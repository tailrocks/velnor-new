//! Independent anonymous bun source owner; repository execution never enters it.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CacheMode, CacheSnapshotDomain, CompiledSourceHelper, Job, JobTimeout, PermissionLevel,
    Permissions, SourceProducer, SourceProducerRole, Step, StepId, StepKind, ToolCacheDescriptor,
    ToolCacheDomain, ToolProducerSelection,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog, catalog::qualification::DistributionHost};
use velnor_actions_workflow_renderer::MiseSetup;

use crate::{OrchestratorError, workloads::cache_eligibility::NativeNpmSource};

const RESTORE_ID: &str = "velnor-bun-cache";

/// Canonical source tuples, exact native schema and platform own compatibility.
pub(crate) fn source_key(
    sources: &[NativeNpmSource],
    catalog: &ToolCatalog,
    label: &str,
) -> Result<String, OrchestratorError> {
    let mut sources = sources.to_vec();
    sources.sort();
    sources.dedup();
    let descriptors = serde_json::to_string(&sources).map_err(contract_error)?;
    Ok(format!(
        "{}{}",
        compatibility_prefix(catalog, label)?,
        velnor_actions_contract::digest_b3(descriptors.as_bytes())
    ))
}

fn compatibility_prefix(catalog: &ToolCatalog, label: &str) -> Result<String, OrchestratorError> {
    let host = host_for_label(label)?;
    let identity = format!(
        "bun-anonymous-source-v1\n{label}\n{}\nbun={}\nnative-extracted-v1\n{}",
        catalog
            .native_tool_specs(host, &[PinnedTool::Bun])?
            .join("\n"),
        velnor_actions_mise::catalog::BUN_VERSION,
        super::bun::producer::source_schema_digest(env!("CARGO_PKG_VERSION"))?,
    );
    Ok(format!(
        "velnor-native-v3-bun-downloads-{}-",
        velnor_actions_contract::digest_b3(identity.as_bytes())
    ))
}

/// Distinct source/platform cohorts share one optional pure producer.
pub(crate) fn producer_id(key: &str) -> String {
    format!(
        "bun-source-{}",
        velnor_actions_contract::digest_b3(key.as_bytes())
    )
}

/// Exact authority records registered alongside the generated producer cohort.
pub(crate) fn source_records(
    sources: &[NativeNpmSource],
    catalog: &ToolCatalog,
    label: &str,
    mise: &MiseSetup,
    version: &str,
    selection: &ToolProducerSelection,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    validate_runtime_version(version)?;
    let key = source_key(sources, catalog, label)?;
    let prepare = prepare_record(catalog, label, version)?;
    let descriptor = tool_descriptor(&prepare, label, mise)?;
    Ok(vec![
        prepare,
        producer_record(sources, &key, version)?,
        super::source_snapshot::record(CacheSnapshotDomain::BunDownloads, true, version)?,
        super::source_snapshot::record(CacheSnapshotDomain::BunDownloads, false, version)?,
        super::source_report::record(&source_metadata(&key, selection, &descriptor)?, version)?,
    ])
}

/// Producer allocation is limited to a trusted default-branch head.
/// Callers attach the same pre-allocation admission as the selected consumer.
pub(crate) fn producer_job(
    sources: &[NativeNpmSource],
    catalog: &ToolCatalog,
    label: &str,
    mise: &MiseSetup,
    version: &str,
    selection: &ToolProducerSelection,
) -> Result<Job, OrchestratorError> {
    validate_runtime_version(version)?;
    if sources.is_empty() {
        return Err(contract_error("bun_source_producer_without_candidates"));
    }
    let key = source_key(sources, catalog, label)?;
    let prepare = prepare_record(catalog, label, version)?;
    let descriptor = tool_descriptor(&prepare, label, mise)?;
    let metadata = source_metadata(&key, selection, &descriptor)?;
    let mut before =
        super::source_snapshot::step(CacheSnapshotDomain::BunDownloads, true, version)?;
    before.id = Some(StepId::new("velnor-bun-source-before").map_err(contract_error)?);
    let mut after =
        super::source_snapshot::step(CacheSnapshotDomain::BunDownloads, false, version)?;
    after.id = Some(StepId::new("velnor-bun-source-after").map_err(contract_error)?);
    after.condition = Some(public_gate());
    let mut steps = velnor_actions_workflow_renderer::tool_producer_steps::tool_consumer_steps(
        &descriptor,
        mise,
    )?;
    steps.extend([
        prepare_bun(&prepare)?,
        producer_restore_step(&key, &compatibility_prefix(catalog, label)?)?,
        before,
        producer_step(sources, &key, version)?,
        after,
        save_step(&metadata)?,
        super::source_report::publication_step(&metadata, &super::bun::payload_paths())?,
        super::source_report::step(&metadata, version)?,
    ]);
    Ok(Job {
        cache_mode: Some(CacheMode::Write),
        display_name: "Public bun sources".to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: selection.needs(ToolCacheDomain::BunBootstrap),
        condition: Some(metadata.condition()),
        permissions: Some(Permissions {
            contents: PermissionLevel::None,
            actions: PermissionLevel::None,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            issues: PermissionLevel::None,
            pages: PermissionLevel::None,
            attestations: PermissionLevel::None,
        }),
        environment: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: Some(metadata.clone()),
        native_pages_deploy: None,
        native_publish: None,
        outputs: super::source_report::outputs(&metadata),
        steps,
    })
}

fn source_metadata(
    key: &str,
    selection: &ToolProducerSelection,
    descriptor: &ToolCacheDescriptor,
) -> Result<SourceProducer, OrchestratorError> {
    selection.validate().map_err(contract_error)?;
    if selection.tasks.is_empty() || selection.cargo_fallback || selection.unconditional {
        return Err(contract_error("bun_source_selection_not_explicit"));
    }
    Ok(SourceProducer {
        selection: selection.clone(),
        tool_cache: Some(descriptor.clone()),
        role: SourceProducerRole::Bun,
        source_identity: key.to_owned(),
        restore_step: StepId::new(RESTORE_ID).map_err(contract_error)?,
        verification_step: StepId::new("velnor-bun-public-proof").map_err(contract_error)?,
        save_step: StepId::new("velnor-bun-source-save").map_err(contract_error)?,
        publication_step: StepId::new("velnor-bun-source-publication").map_err(contract_error)?,
        report_step: StepId::new(super::source_report::REPORT_ID).map_err(contract_error)?,
    })
}

/// Read only source-compatible immutable snapshots; never unrelated lock sets.
pub(crate) fn restore_step(key: &str) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::action_step(
        "Restore bun_downloads",
        velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES,
        BTreeMap::from([
            ("path".to_owned(), super::bun::payload_paths().join("\n")),
            ("key".to_owned(), format!("{key}-lookup")),
            ("restore-keys".to_owned(), format!("{key}-snapshot-")),
        ]),
    )?;
    step.id = Some(StepId::new(RESTORE_ID).map_err(contract_error)?);
    Ok(step)
}

fn producer_restore_step(key: &str, compatible: &str) -> Result<Step, OrchestratorError> {
    let mut step = restore_step(key)?;
    if let StepKind::Action { with, .. } = &mut step.kind {
        with.insert(
            "restore-keys".to_owned(),
            format!("{key}-snapshot-\n{compatible}"),
        );
    }
    Ok(step)
}

fn save_step(metadata: &SourceProducer) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::action_step(
        "Save bun_downloads",
        velnor_actions_workflow_renderer::steps::TOOLS_SAVE_USES,
        BTreeMap::from([
            ("path".to_owned(), super::bun::payload_paths().join("\n")),
            ("key".to_owned(), metadata.save_key()),
        ]),
    )?;
    step.id = Some(metadata.save_step.clone());
    step.condition = Some(metadata.save_condition());
    Ok(step)
}

fn prepare_bun(record: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Prepare isolated bun source tools",
        record,
        record.environment().clone(),
    )?;
    step.id = Some(StepId::new("velnor-bun-source-prepare").map_err(contract_error)?);
    Ok(step)
}

/// Exact host and selectors are admitted by the canonical installation owner.
pub(crate) fn prepare_record(
    catalog: &ToolCatalog,
    label: &str,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let target = tool_target(label)?;
    let host = match target {
        "x86_64-unknown-linux-gnu" => DistributionHost::LinuxAmd64,
        "aarch64-unknown-linux-gnu" => DistributionHost::LinuxArm64,
        "aarch64-apple-darwin" => DistributionHost::MacosArm64,
        _ => return Err(contract_error("bun_source_tool_host_unqualified")),
    };
    let selectors = catalog.native_tool_specs(host, &[PinnedTool::Bun])?;
    velnor_actions_mise::catalog::tool_prepare::helper_for_tools(
        catalog,
        ToolCacheDomain::BunBootstrap,
        host,
        &selectors,
        version,
    )
    .map_err(contract_error)
}

fn host_for_label(label: &str) -> Result<DistributionHost, OrchestratorError> {
    match tool_target(label)? {
        "x86_64-unknown-linux-gnu" => Ok(DistributionHost::LinuxAmd64),
        "aarch64-unknown-linux-gnu" => Ok(DistributionHost::LinuxArm64),
        "aarch64-apple-darwin" => Ok(DistributionHost::MacosArm64),
        _ => Err(contract_error("bun_source_tool_host_unqualified")),
    }
}

fn tool_descriptor(
    record: &CompiledSourceHelper,
    label: &str,
    mise: &MiseSetup,
) -> Result<ToolCacheDescriptor, OrchestratorError> {
    Ok(
        velnor_actions_workflow_renderer::tool_producer_steps::descriptor_for_record(
            record,
            label,
            tool_target(label)?,
            ToolCacheDomain::BunBootstrap,
            mise,
            std::slice::from_ref(record),
        )?,
    )
}

fn tool_target(label: &str) -> Result<&'static str, OrchestratorError> {
    velnor_actions_contract::tool_target_for_runner_label(label)
        .ok_or_else(|| contract_error("bun_source_tool_runner_unqualified"))
}

fn producer_record(
    sources: &[NativeNpmSource],
    key: &str,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let env = BTreeMap::from([("VELNOR_SOURCE_IDENTITY".to_owned(), key.to_owned())]);
    Ok(super::bun::producer::compiled_helper(sources, version)?.with_environment(env))
}

fn producer_step(
    sources: &[NativeNpmSource],
    key: &str,
    version: &str,
) -> Result<Step, OrchestratorError> {
    let record = producer_record(sources, key, version)?;
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Verify public Bun sources",
        &record,
        record.environment().clone(),
    )?;
    step.id = Some(StepId::new("velnor-bun-public-proof").map_err(contract_error)?);
    Ok(step)
}

fn public_gate() -> String {
    "steps.velnor-bun-public-proof.outputs.verified == 'true'".to_owned()
}

fn validate_runtime_version(version: &str) -> Result<(), OrchestratorError> {
    if version != env!("CARGO_PKG_VERSION") {
        return Err(contract_error("bun_source_runtime_version_unqualified"));
    }
    Ok(())
}

fn contract_error(error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
#[path = "workloads_cache_bun_source_job_tests.rs"]
mod tests;
