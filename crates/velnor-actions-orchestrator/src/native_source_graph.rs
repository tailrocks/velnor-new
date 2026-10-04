//! Optional anonymous native source cohorts derive solely from immutable tasks.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::tool_producer_selection::ToolProducerSelection;
use velnor_actions_contract::{CompiledSourceHelper, Job, SourceProducerRole, VelnorConfig};
use velnor_actions_mise::ToolCatalog;

use crate::workloads::cache_eligibility::NativeNpmSource;
use crate::{OrchestratorError, discover::Discovery, workloads::cache};

#[path = "native_source_graph_admission.rs"]
mod admission;

pub(crate) struct NativeSourceBuild {
    pub(crate) source_helpers: Vec<CompiledSourceHelper>,
    pub(crate) receipt_drafts: Vec<crate::cache_producer_workflow::DraftCacheProducerWorkflow>,
}

struct Cohort {
    role: SourceProducerRole,
    key: String,
    label: String,
    catalog: ToolCatalog,
    sources: Vec<NativeNpmSource>,
    consumers: Vec<String>,
    selection: ToolProducerSelection,
}

/// Bind read-only consumers to one pure producer per exact source/platform set.
pub(crate) fn insert_producers(
    jobs: &mut BTreeMap<String, Job>,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    config: &VelnorConfig,
    version: &str,
) -> Result<NativeSourceBuild, OrchestratorError> {
    let cohorts = collect_cohorts(jobs, &discovery.proposals, catalog)?;
    let mut records = Vec::new();
    let mut receipt_drafts = Vec::new();
    for (id, cohort) in cohorts {
        if jobs.contains_key(&id) {
            return Err(invalid("producer_collision"));
        }
        let mise = crate::pins::resolve_mise_setup(config, &cohort.label)?;
        let Some(prepared) = admission::admit_cohort(&cohort, &mise, version)? else {
            continue;
        };
        for consumer_id in &cohort.consumers {
            let consumer = jobs
                .get_mut(consumer_id)
                .ok_or_else(|| invalid("consumer_missing"))?;
            attach_restore(consumer, &cohort)?;
            cache::source_report::depend_on_producer(consumer, &id);
        }
        records.extend(prepared.records);
        receipt_drafts.push(prepared.draft);
        jobs.insert(id, prepared.producer);
    }
    Ok(NativeSourceBuild {
        source_helpers: records,
        receipt_drafts,
    })
}

fn collect_cohorts(
    jobs: &BTreeMap<String, Job>,
    proposals: &[velnor_actions_contract::ProposedTask],
    catalog: &ToolCatalog,
) -> Result<BTreeMap<String, Cohort>, OrchestratorError> {
    let grouped = crate::crate_job_ids::group_runnable(proposals);
    let assigned = crate::crate_job_ids::assign_group_ids(&grouped);
    let mut cohorts: BTreeMap<String, Cohort> = BTreeMap::new();
    for (group, tasks) in grouped {
        let role = match group.1.as_str() {
            "node_ci" => SourceProducerRole::Npm,
            // Bun materialized caches need the admitted payload/receipt guard
            // before their consumers can restore; no orphan producer is emitted.
            "bun_ci" => continue,
            _ => continue,
        };
        let sources = crate::workloads::cache_sources::native_sources_for_tasks(&tasks)?;
        if sources.is_empty() {
            continue;
        }
        let consumer_id = assigned
            .get(&group)
            .ok_or_else(|| invalid("consumer_id_missing"))?;
        let consumer = jobs
            .get(consumer_id)
            .ok_or_else(|| invalid("consumer_unemitted"))?;
        let scoped = catalog.clone();
        let (key, id) = source_identity(role, &sources, &scoped, &consumer.runs_on)?;
        let mut selected_tasks: Vec<String> =
            tasks.iter().map(|task| task.task_id.clone()).collect();
        selected_tasks.sort();
        selected_tasks.dedup();
        if let Some(cohort) = cohorts.get_mut(&id) {
            if cohort.role != role
                || cohort.key != key
                || cohort.label != consumer.runs_on
                || cohort.sources != sources
            {
                return Err(invalid("cohort_identity_collision"));
            }
            cohort.consumers.push(consumer_id.clone());
            cohort.selection.tasks.extend(selected_tasks);
            cohort.selection.tasks.sort();
            cohort.selection.tasks.dedup();
        } else {
            cohorts.insert(
                id,
                Cohort {
                    role,
                    key,
                    label: consumer.runs_on.clone(),
                    catalog: scoped,
                    sources,
                    consumers: vec![consumer_id.clone()],
                    selection: ToolProducerSelection {
                        tasks: selected_tasks,
                        cargo_fallback: false,
                        unconditional: false,
                    },
                },
            );
        }
    }
    Ok(cohorts)
}

fn source_identity(
    role: SourceProducerRole,
    sources: &[NativeNpmSource],
    catalog: &ToolCatalog,
    label: &str,
) -> Result<(String, String), OrchestratorError> {
    let (key, id) = match role {
        SourceProducerRole::Npm => {
            let key = cache::npm_source_job::source_key(sources, catalog, label)?;
            let id = cache::npm_source_job::producer_id(&key);
            (key, id)
        }
        SourceProducerRole::Bun => {
            let key = cache::bun_source_job::source_key(sources, catalog, label)?;
            let id = cache::bun_source_job::producer_id(&key);
            (key, id)
        }
        _ => return Err(invalid("unsupported_role")),
    };
    Ok((key, id))
}

fn attach_restore(consumer: &mut Job, cohort: &Cohort) -> Result<(), OrchestratorError> {
    let restore = match cohort.role {
        SourceProducerRole::Npm => cache::npm_source_job::restore_step(&cohort.key)?,
        SourceProducerRole::Bun => cache::bun_source_job::restore_step(&cohort.key)?,
        _ => return Err(invalid("unsupported_role")),
    };
    if consumer.steps.iter().any(|step| step == &restore) {
        return Err(invalid("source_restore_already_attached"));
    }
    let anchor = crate::matrix_step::download_plan_step()?;
    let at = consumer
        .steps
        .iter()
        .position(|step| step == &anchor)
        .ok_or_else(|| invalid("consumer_source_anchor_missing"))?;
    consumer.steps.insert(at + 1, restore);
    Ok(())
}

fn source_records(
    cohort: &Cohort,
    catalog: &ToolCatalog,
    mise: &velnor_actions_workflow_renderer::MiseSetup,
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    match cohort.role {
        SourceProducerRole::Npm => cache::npm_source_job::source_records(
            &cohort.sources,
            catalog,
            &cohort.label,
            mise,
            version,
            &cohort.selection,
        ),
        SourceProducerRole::Bun => cache::bun_source_job::source_records(
            &cohort.sources,
            catalog,
            &cohort.label,
            mise,
            version,
            &cohort.selection,
        ),
        _ => Err(invalid("unsupported_role")),
    }
}

fn validate_bootstrap(
    cohort: &Cohort,
    mise: &velnor_actions_workflow_renderer::MiseSetup,
    version: &str,
    metadata: &velnor_actions_contract::SourceProducer,
    records: &[CompiledSourceHelper],
) -> Result<(), OrchestratorError> {
    let (record, domain) = match cohort.role {
        SourceProducerRole::Npm => (
            cache::npm_source_job::prepare_record(&cohort.catalog, &cohort.label, version)?,
            velnor_actions_contract::ToolCacheDomain::NpmBootstrap,
        ),
        SourceProducerRole::Bun => (
            cache::bun_source_job::prepare_record(&cohort.catalog, &cohort.label, version)?,
            velnor_actions_contract::ToolCacheDomain::BunBootstrap,
        ),
        _ => return Err(invalid("unsupported_role")),
    };
    let target = velnor_actions_contract::tool_target_for_runner_label(&cohort.label)
        .ok_or_else(|| invalid("bootstrap_target_unqualified"))?;
    let expected = velnor_actions_workflow_renderer::tool_producer_steps::descriptor_for_record(
        &record,
        &cohort.label,
        target,
        domain,
        mise,
        records,
    )?;
    if metadata.tool_cache.as_ref() != Some(&expected) {
        return Err(invalid("bootstrap_identity_mismatch"));
    }
    Ok(())
}

fn invalid(reason: &str) -> OrchestratorError {
    crate::internal::internal(&format!("native_source_graph:{reason}"))
}

#[cfg(test)]
#[path = "native_source_graph_tests.rs"]
mod tests;
