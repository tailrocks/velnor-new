//! Independent anonymous npm source owner; repository execution never enters it.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CacheMode, CacheSnapshotDomain, CompiledSourceHelper, Job, JobTimeout, PermissionLevel,
    Permissions, SourceProducer, SourceProducerRole, Step, StepId, StepKind, ToolCacheDescriptor,
    ToolCacheDomain, ToolProducerSelection,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog, catalog::qualification::DistributionHost};
use velnor_actions_workflow_renderer::MiseSetup;

use crate::{OrchestratorError, workloads::cache_eligibility::NativeNpmSource};

const DATA: &str = "${{ runner.temp }}/velnor/npm-source/mise";
const RESTORE_ID: &str = "velnor-npm-cache";

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
    let record = super::npm_proof::source_record(
        &[],
        catalog,
        host,
        &node_binary(catalog, host)?,
        "compatibility-schema",
        env!("CARGO_PKG_VERSION"),
    )?;
    let identity = format!(
        "npm-anonymous-source-v1\n{label}\n{}\nnode={}\nnpm={}\nmodule_abi={}\n\
         cacache/content-v2\npublic-proof-v1\n{}",
        catalog
            .native_tool_specs(host, &[PinnedTool::Node])?
            .join("\n"),
        velnor_actions_mise::catalog::NODE_VERSION,
        velnor_actions_mise::catalog::NODE_NPM_VERSION,
        velnor_actions_mise::catalog::NODE_MODULE_ABI_VERSION,
        record.invocation().descriptor().source_sha256(),
    );
    Ok(format!(
        "velnor-native-v3-npm-downloads-{}-",
        velnor_actions_contract::digest_b3(identity.as_bytes())
    ))
}

/// Distinct source/platform cohorts share one optional pure producer.
pub(crate) fn producer_id(key: &str) -> String {
    format!(
        "npm-source-{}",
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
    verify_runtime_version(version)?;
    let key = source_key(sources, catalog, label)?;
    let host = host_for_label(label)?;
    let node = node_binary(catalog, host)?;
    let prepare = prepare_record(catalog, label, version)?;
    let descriptor = tool_descriptor(&prepare, label, mise)?;
    Ok(vec![
        prepare,
        super::npm_proof::source_record(sources, catalog, host, &node, &key, version)?,
        super::source_snapshot::record(CacheSnapshotDomain::NpmDownloads, true, version)?,
        super::source_snapshot::record(CacheSnapshotDomain::NpmDownloads, false, version)?,
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
    verify_runtime_version(version)?;
    if sources.is_empty() {
        return Err(contract_error("npm_source_producer_without_candidates"));
    }
    let key = source_key(sources, catalog, label)?;
    let prepare = prepare_record(catalog, label, version)?;
    let descriptor = tool_descriptor(&prepare, label, mise)?;
    let metadata = source_metadata(&key, selection, &descriptor)?;
    let host = host_for_label(label)?;
    let node = node_binary(catalog, host)?;
    let mut before =
        super::source_snapshot::step(CacheSnapshotDomain::NpmDownloads, true, version)?;
    before.id = Some(StepId::new("velnor-npm-source-before").map_err(contract_error)?);
    let mut after =
        super::source_snapshot::step(CacheSnapshotDomain::NpmDownloads, false, version)?;
    after.id = Some(StepId::new("velnor-npm-source-after").map_err(contract_error)?);
    after.condition = Some(public_gate());
    let mut steps = velnor_actions_workflow_renderer::tool_producer_steps::tool_consumer_steps(
        &descriptor,
        mise,
    )?;
    steps.extend([
        prepare_node(&prepare)?,
        producer_restore_step(&key, &compatibility_prefix(catalog, label)?)?,
        before,
        super::npm_proof::source_step(sources, catalog, host, &node, &key, version)?,
        after,
        save_step(&metadata)?,
        super::source_report::publication_step(&metadata, &super::npm::payload_paths())?,
        super::source_report::step(&metadata, version)?,
    ]);
    Ok(Job {
        display_name: "Public npm sources".to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: selection.needs(ToolCacheDomain::NpmBootstrap),
        condition: Some(metadata.condition()),
        cache_mode: Some(CacheMode::Write),
        permissions: Some(Permissions {
            attestations: PermissionLevel::None,
            contents: PermissionLevel::None,
            actions: PermissionLevel::None,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            issues: PermissionLevel::None,
            pages: PermissionLevel::None,
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
    let metadata = SourceProducer {
        role: SourceProducerRole::Npm,
        selection: selection.clone(),
        tool_cache: Some(descriptor.clone()),
        source_identity: key.to_owned(),
        restore_step: StepId::new(RESTORE_ID).map_err(contract_error)?,
        verification_step: StepId::new("velnor-npm-public-proof").map_err(contract_error)?,
        save_step: StepId::new("velnor-npm-source-save").map_err(contract_error)?,
        publication_step: StepId::new("velnor-npm-source-publication").map_err(contract_error)?,
        report_step: StepId::new(super::source_report::REPORT_ID).map_err(contract_error)?,
    };
    metadata.validate()?;
    Ok(metadata)
}

/// Read only source-compatible immutable snapshots; never unrelated lock sets.
pub(crate) fn restore_step(key: &str) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::action_step(
        "Restore npm_downloads",
        velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES,
        BTreeMap::from([
            ("path".to_owned(), super::npm::payload_paths().join("\n")),
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

fn node_binary(catalog: &ToolCatalog, host: DistributionHost) -> Result<String, OrchestratorError> {
    let distribution = catalog.native_distribution(host, PinnedTool::Node)?;
    let _plan = distribution.required_install_plan()?;
    let path = distribution.required_installed_binary_path()?;
    Ok(format!("{DATA}/{path}"))
}

fn save_step(metadata: &SourceProducer) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::action_step(
        "Save npm_downloads",
        velnor_actions_workflow_renderer::steps::TOOLS_SAVE_USES,
        BTreeMap::from([
            ("path".to_owned(), super::npm::payload_paths().join("\n")),
            ("key".to_owned(), metadata.save_key()),
        ]),
    )?;
    step.id = Some(metadata.save_step.clone());
    step.condition = Some(metadata.save_condition());
    Ok(step)
}

fn prepare_node(record: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Prepare isolated npm source tools",
        record,
        record.environment().clone(),
    )?;
    step.id = Some(StepId::new("velnor-npm-source-prepare")?);
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
        _ => return Err(contract_error("npm_source_tool_host_unqualified")),
    };
    let selectors = catalog.native_tool_specs(host, &[PinnedTool::Node])?;
    velnor_actions_mise::catalog::tool_prepare::helper_for_tools(
        catalog,
        ToolCacheDomain::NpmBootstrap,
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
        _ => Err(contract_error("npm_source_tool_host_unqualified")),
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
            ToolCacheDomain::NpmBootstrap,
            mise,
            std::slice::from_ref(record),
        )?,
    )
}

fn tool_target(label: &str) -> Result<&'static str, OrchestratorError> {
    velnor_actions_contract::tool_target_for_runner_label(label)
        .ok_or_else(|| contract_error("npm_source_tool_runner_unqualified"))
}

fn public_gate() -> String {
    "steps.velnor-npm-public-proof.outputs.verified == 'true'".to_owned()
}

fn verify_runtime_version(version: &str) -> Result<(), OrchestratorError> {
    if version != env!("CARGO_PKG_VERSION") {
        return Err(contract_error("npm_source_runtime_version_mismatch"));
    }
    Ok(())
}

fn contract_error(error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
#[path = "workloads_cache_npm_source_job_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workloads_cache_npm_source_transport_tests.rs"]
mod transport_tests;
