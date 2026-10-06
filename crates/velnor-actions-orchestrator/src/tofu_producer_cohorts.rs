//! Exact root and source admission for public provider producer cohorts.
use super::{PendingProducer, producer_id, selection_for_tasks, source_key, tofu_root_for_tasks};
use crate::{OrchestratorError, discover::Discovery};
use std::collections::BTreeMap;
use velnor_actions_contract::{Job, VelnorConfig};
use velnor_actions_mise::ToolCatalog;

pub(super) fn collect(
    jobs: &mut BTreeMap<String, Job>,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    config: &VelnorConfig,
) -> Result<BTreeMap<String, PendingProducer>, OrchestratorError> {
    let grouped = crate::crate_job_ids::group_runnable(&discovery.proposals);
    let assigned = crate::crate_job_ids::assign_group_ids(&grouped);
    let mut pending: BTreeMap<String, PendingProducer> = BTreeMap::new();
    for ((unit, configuration), tasks) in grouped {
        if !crate::crate_job_ids::group_is_tofu(&tasks) {
            continue;
        }
        let Some(descriptor) = crate::tofu_cache_source::descriptor_for_tasks(&tasks) else {
            continue;
        };
        let consumer_id = assigned
            .get(&(unit, configuration.clone()))
            .ok_or_else(|| crate::internal::internal("tofu_consumer_job_missing"))?;
        let consumer_label = jobs
            .get(consumer_id)
            .ok_or_else(|| crate::internal::internal("tofu_consumer_job_unemitted"))?
            .runs_on
            .clone();
        let root = tofu_root_for_tasks(&tasks)?;
        let mise = crate::pins::resolve_mise_setup(config, &consumer_label)?;
        let key = source_key(&descriptor, catalog, &consumer_label, &root)?;
        let producer_job_id = producer_id(&key);
        let selection = selection_for_tasks(&tasks);
        if let Some(existing) = pending.get_mut(&producer_job_id) {
            if existing.descriptor != descriptor
                || existing.label != consumer_label
                || existing.root != root
            {
                return Err(crate::internal::internal(
                    "tofu_producer_identity_collision",
                ));
            }
            existing.selection.tasks.extend(selection.tasks);
            existing.selection.tasks.sort();
            existing.selection.tasks.dedup();
        } else {
            pending.insert(
                producer_job_id.clone(),
                PendingProducer {
                    descriptor,
                    label: consumer_label,
                    root,
                    mise,
                    selection,
                },
            );
        }
        let consumer = jobs
            .get_mut(consumer_id)
            .ok_or_else(|| crate::internal::internal("tofu_consumer_job_unemitted"))?;
        crate::workloads::cache::source_report::depend_on_producer(consumer, &producer_job_id);
    }
    Ok(pending)
}
