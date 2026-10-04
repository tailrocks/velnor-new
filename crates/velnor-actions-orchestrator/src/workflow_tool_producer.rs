//! Isolated tool production replaces every computation-job cache writer.
use crate::{OrchestratorError, discover::Discovery};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, Job, JobTimeout, PermissionLevel, Permissions, PureToolProducer, StepId,
    ToolCacheDescriptor, ToolCacheDomain, ToolProducerSelection,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::MiseSetup;

#[path = "workflow_tool_consumer_prepare.rs"]
mod consumer_prepare;
#[path = "workflow_tool_dependencies.rs"]
mod dependencies;
#[path = "workflow_tool_selection.rs"]
mod selection;

struct Cohort {
    descriptor: ToolCacheDescriptor,
    consumers: Vec<String>,
}

/// Insert pure producers and bind consumers to the exact compiled tool domains.
pub(crate) fn insert_producers(
    jobs: &mut BTreeMap<String, Job>,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    setup: &MiseSetup,
    version: &str,
    owner_records: &[CompiledSourceHelper],
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let mut records = consumer_prepare::normalize(jobs, catalog, version)?;
    let mut authority = owner_records.to_vec();
    authority.extend(records.iter().cloned());
    authority.extend(
        setup
            .bootstraps
            .values()
            .map(|bootstrap| bootstrap.helper.clone()),
    );
    let cohorts = collect_cohorts(jobs, setup, &authority)?;
    let task_index = selection::task_index(discovery);
    let mut pending = Vec::new();
    for (id, cohort) in cohorts {
        let selection = if cohort.descriptor.domain == ToolCacheDomain::Planning {
            selection::Selection::default()
        } else {
            selection::for_consumers(&cohort.consumers, jobs, &task_index)
        };
        let selection = ToolProducerSelection {
            tasks: selection.tasks.into_iter().collect(),
            cargo_fallback: selection.cargo_fallback,
            unconditional: selection.unconditional,
        };
        let (producer, record) =
            producer_job(&cohort.descriptor, catalog, setup, version, selection)?;
        for consumer in &cohort.consumers {
            // Plan's local fallback precedes full production; this edge would cycle.
            if consumer == "plan" && cohort.descriptor.domain != ToolCacheDomain::Planning {
                continue;
            }
            // Required's fan-in separately classifies intentional producer skips.
            if consumer == velnor_actions_workflow_renderer::render::FINAL_JOB_ID {
                continue;
            }
            let consumer = jobs
                .get_mut(consumer)
                .ok_or_else(|| crate::internal::internal("tool_consumer_disappeared"))?;
            dependencies::depend_on_producer(consumer, &id);
        }
        records.extend(record);
        pending.push((id, producer));
    }
    let required = jobs
        .get_mut(velnor_actions_workflow_renderer::render::FINAL_JOB_ID)
        .ok_or_else(|| crate::internal::internal("tool_required_job_missing"))?;
    required
        .needs
        .extend(pending.iter().map(|(id, _)| id.clone()));
    required.needs.sort();
    required.needs.dedup();
    for (id, job) in pending {
        if jobs.insert(id, job).is_some() {
            return Err(crate::internal::internal("tool_producer_job_collision"));
        }
    }
    Ok(records)
}

fn collect_cohorts(
    jobs: &BTreeMap<String, Job>,
    setup: &MiseSetup,
    records: &[CompiledSourceHelper],
) -> Result<BTreeMap<String, Cohort>, OrchestratorError> {
    let mut cohorts: BTreeMap<String, Cohort> = BTreeMap::new();
    for (id, job) in jobs {
        let target = velnor_actions_contract::tool_target_for_runner_label(&job.runs_on)
            .ok_or_else(|| crate::internal::internal("tool_consumer_target_unknown"))?;
        let descriptors =
            velnor_actions_workflow_renderer::tool_producer_steps::consumer_tool_descriptors(
                id, job, setup, target, records,
            )?;
        for descriptor in descriptors {
            let producer = producer_id(&descriptor)?;
            if let Some(cohort) = cohorts.get_mut(&producer) {
                if cohort.descriptor != descriptor {
                    return Err(crate::internal::internal(
                        "tool_descriptor_identity_collision",
                    ));
                }
                cohort.consumers.push(id.clone());
            } else {
                cohorts.insert(
                    producer,
                    Cohort {
                        descriptor,
                        consumers: vec![id.clone()],
                    },
                );
            }
        }
    }
    Ok(cohorts)
}

fn producer_id(descriptor: &ToolCacheDescriptor) -> Result<String, OrchestratorError> {
    let canonical = velnor_actions_contract::canonical_json_str(descriptor)?;
    Ok(format!(
        "tools-{}-{}",
        descriptor.domain.name(),
        velnor_actions_contract::digest_b3(canonical.as_bytes())
    ))
}

/// Build one exact source-qualified producer without checkout or repository execution.
pub(crate) fn producer_job(
    descriptor: &ToolCacheDescriptor,
    catalog: &ToolCatalog,
    setup: &MiseSetup,
    version: &str,
    selection: ToolProducerSelection,
) -> Result<(Job, Vec<CompiledSourceHelper>), OrchestratorError> {
    let metadata = metadata(descriptor, selection)?;
    let scoped_catalog = catalog_for_descriptor(catalog, descriptor)?;
    let installation = velnor_actions_mise::catalog::tool_prepare::helper_for_tools(
        &scoped_catalog,
        descriptor.domain,
        distribution_host(&descriptor.target)?,
        &descriptor.selectors,
        version,
    )
    .map_err(|error| OrchestratorError::Contract {
        problem: error.to_string(),
    })?;
    let layer = velnor_actions_contract::CacheSnapshotDomain::tool_domain(descriptor.domain);
    let before = crate::workloads::cache::source_snapshot::record(layer, true, version)?;
    let after = crate::workloads::cache::source_snapshot::record(layer, false, version)?;
    let steps = velnor_actions_workflow_renderer::cache_p08::producer_steps(
        &metadata,
        setup,
        &installation,
        &before,
        &after,
        version,
    )?;
    let report = velnor_actions_workflow_renderer::cache_p08::report_record(&metadata, version)?;
    let acquisition = setup
        .bootstrap(descriptor.domain, &descriptor.runs_on)?
        .helper
        .clone();
    let condition = metadata.selection.condition(descriptor.domain);
    let needs = metadata.selection.needs(descriptor.domain);
    Ok((
        Job {
            display_name: format!("Prepare {} tools", descriptor.domain.name()),
            runs_on: descriptor.runs_on.clone(),
            timeout_minutes: JobTimeout::CRATE,
            needs,
            condition: Some(condition),
            cache_mode: Some(velnor_actions_contract::CacheMode::Write),
            outputs: velnor_actions_workflow_renderer::cache_p08::producer_outputs(&metadata),
            permissions: Some(empty_permissions()),
            environment: None,
            source_producer: None,
            native_pages_deploy: None,
            native_publish: None,
            tool_producer: Some(metadata),
            mbx_producer: None,
            steps,
        },
        vec![acquisition, before, installation, after, report],
    ))
}

fn catalog_for_descriptor(
    catalog: &ToolCatalog,
    descriptor: &ToolCacheDescriptor,
) -> Result<ToolCatalog, OrchestratorError> {
    if descriptor.selectors.contains(
        &catalog
            .tool_spec(catalog.compiler_tool())
            .map_err(mise_error)?,
    ) {
        return Ok(catalog.clone());
    }
    for scoped in [
        catalog.for_native_kind("native_xcode_project_ci"),
        catalog.for_native_source_kind("native_xcode_project_ci"),
    ] {
        let scoped = scoped.map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
        if descriptor.selectors.contains(
            &scoped
                .tool_spec(scoped.compiler_tool())
                .map_err(mise_error)?,
        ) {
            return Ok(scoped);
        }
    }
    Ok(catalog.clone())
}

fn distribution_host(
    target: &str,
) -> Result<velnor_actions_mise::catalog::qualification::DistributionHost, OrchestratorError> {
    use velnor_actions_mise::catalog::qualification::DistributionHost;
    match target {
        "x86_64-unknown-linux-gnu" => Ok(DistributionHost::LinuxAmd64),
        "aarch64-unknown-linux-gnu" => Ok(DistributionHost::LinuxArm64),
        "aarch64-apple-darwin" => Ok(DistributionHost::MacosArm64),
        _ => Err(crate::internal::internal("tool_distribution_host_unknown")),
    }
}

fn metadata(
    descriptor: &ToolCacheDescriptor,
    selection: ToolProducerSelection,
) -> Result<PureToolProducer, OrchestratorError> {
    Ok(PureToolProducer {
        descriptor: descriptor.clone(),
        selection,
        restore_step: StepId::new(
            velnor_actions_contract::CacheSnapshotDomain::tool_domain(descriptor.domain)
                .restore_id(),
        )?,
        before_step: StepId::new("velnor-tool-before")?,
        installation_step: StepId::new("velnor-tool-install")?,
        after_step: StepId::new("velnor-tool-after")?,
        save_step: StepId::new("velnor-tool-save")?,
        report_step: StepId::new("velnor-tool-report")?,
    })
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

fn mise_error(error: velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
#[path = "workflow_tool_producer_tests.rs"]
mod tests;
