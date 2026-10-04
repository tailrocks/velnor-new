//! Closed pure Rust source cohorts; computation jobs remain read-only.

use std::{collections::BTreeMap, path::Path};

use velnor_actions_contract::{CompiledSourceHelper, Job, Stack, Step, ToolProducerSelection};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::MiseSetup;

use crate::{OrchestratorError, discover::Discovery};

#[path = "source_producer_descriptor.rs"]
mod descriptor;
#[path = "source_producer_job.rs"]
mod job;
#[path = "source_producer_manifest.rs"]
mod manifest;
#[path = "source_producer_reader.rs"]
mod reader;
#[path = "source_producer_receipt.rs"]
mod receipt;
#[path = "source_producer_source.rs"]
mod source;
#[path = "source_producer_transport.rs"]
mod transport;
pub(crate) use descriptor::RustSourceProjection;
pub(crate) use receipt::{record_for_receipt, source_compatibility_projection};

struct Cohort {
    descriptor: descriptor::RustSourceDescriptor,
    label: String,
    consumers: Vec<String>,
    selection: ToolProducerSelection,
}

/// Replace legacy Plan publication with isolated, explicitly selected producers.
pub(crate) fn insert_producers(
    jobs: &mut BTreeMap<String, Job>,
    root: &Path,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    setup: &MiseSetup,
    roots: &[String],
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let mut cohorts = selected_cohorts(jobs, root, discovery, catalog, roots, version)?;
    add_plan_fallback(&mut cohorts, jobs, root, discovery, catalog, roots, version)?;
    let mut records = Vec::new();
    for (id, cohort) in cohorts {
        let (producer, helpers) = job::producer_job(
            &cohort.descriptor,
            catalog,
            &cohort.label,
            setup,
            version,
            &cohort.selection,
        )?;
        let meta = producer
            .source_producer
            .as_ref()
            .ok_or_else(|| crate::internal::internal("rust_source_role_missing"))?;
        let payload = transport::Source3::new(meta.source_identity.clone())?;
        for consumer_id in &cohort.consumers {
            let consumer = jobs
                .get_mut(consumer_id)
                .ok_or_else(|| crate::internal::internal("rust_source_consumer_missing"))?;
            if let Some(selected) = reader::steps(&cohort.descriptor, catalog)? {
                replace_fetch(consumer, selected);
            }
            let mut restore = payload.restore()?;
            if consumer_id == "plan" {
                velnor_actions_workflow_renderer::early_plan::require_cargo(&mut restore);
            }
            restore_before_fetch(consumer, restore);
            if consumer_id != "plan" {
                crate::workloads::cache::source_report::depend_on_producer(consumer, &id);
            }
        }
        records.extend(helpers);
        if jobs.insert(id, producer).is_some() {
            return Err(crate::internal::internal("rust_source_producer_collision"));
        }
    }
    Ok(records)
}

fn selected_cohorts(
    jobs: &BTreeMap<String, Job>,
    root: &Path,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    roots: &[String],
    version: &str,
) -> Result<BTreeMap<String, Cohort>, OrchestratorError> {
    let grouped = crate::crate_job_ids::group_runnable(&discovery.proposals);
    let assigned = crate::crate_job_ids::assign_group_ids(&grouped);
    let mut cohorts = BTreeMap::new();
    for (group, tasks) in grouped {
        if !tasks.iter().any(|task| task.stack_id == Stack::Rust.id()) {
            continue;
        }
        let id = assigned
            .get(&group)
            .ok_or_else(|| crate::internal::internal("rust_source_assignment_missing"))?;
        let consumer = jobs
            .get(id)
            .ok_or_else(|| crate::internal::internal("rust_source_consumer_missing"))?;
        let selected = crate::source_prep::selected_fetch_roots(discovery, &tasks, roots);
        let selection = ToolProducerSelection {
            tasks: tasks.iter().map(|task| task.task_id.clone()).collect(),
            cargo_fallback: false,
            unconditional: false,
        };
        add_cohort(
            &mut cohorts,
            root,
            discovery,
            catalog,
            &selected,
            &consumer.runs_on,
            id,
            selection,
            version,
            Some(&tasks),
        )?;
    }
    Ok(cohorts)
}

fn add_plan_fallback(
    cohorts: &mut BTreeMap<String, Cohort>,
    jobs: &BTreeMap<String, Job>,
    root: &Path,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    roots: &[String],
    version: &str,
) -> Result<(), OrchestratorError> {
    let Some(plan) = jobs.get("plan") else {
        return Ok(());
    };
    if !velnor_actions_workflow_renderer::early_plan::has_early_plan(plan) {
        return Ok(());
    }
    add_cohort(
        cohorts,
        root,
        discovery,
        catalog,
        roots,
        &plan.runs_on,
        "plan",
        ToolProducerSelection {
            tasks: Vec::new(),
            cargo_fallback: true,
            unconditional: false,
        },
        version,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn add_cohort(
    cohorts: &mut BTreeMap<String, Cohort>,
    root: &Path,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    roots: &[String],
    label: &str,
    consumer: &str,
    mut selection: ToolProducerSelection,
    version: &str,
    tasks: Option<&[&velnor_actions_contract::ProposedTask]>,
) -> Result<(), OrchestratorError> {
    if roots.is_empty() {
        return Ok(());
    }
    let target = velnor_actions_contract::tool_target_for_runner_label(label)
        .ok_or_else(|| crate::internal::internal("rust_source_target_unknown"))?;
    let captured = if let Some(tasks) = tasks {
        descriptor::descriptor_for_tasks(root, roots, discovery, catalog, target, tasks)?
    } else {
        descriptor::descriptor_for(root, roots, discovery, catalog, target)?
    };
    let Some(descriptor) = captured else {
        return Ok(());
    };
    let helper = source::compiled_helper(&descriptor, version)?;
    let identity = descriptor::source_identity(&descriptor, &helper)?;
    let id = format!(
        "rust-source-{}",
        velnor_actions_contract::digest_b3(identity.as_bytes())
    );
    selection.tasks.sort();
    selection.tasks.dedup();
    if let Some(existing) = cohorts.get_mut(&id) {
        if existing.label != label || existing.descriptor != descriptor {
            return Err(crate::internal::internal("rust_source_identity_collision"));
        }
        existing.selection.tasks.extend(selection.tasks);
        existing.selection.tasks.sort();
        existing.selection.tasks.dedup();
        existing.selection.cargo_fallback |= selection.cargo_fallback;
        existing.consumers.push(consumer.to_owned());
    } else {
        cohorts.insert(
            id,
            Cohort {
                descriptor,
                label: label.to_owned(),
                consumers: vec![consumer.to_owned()],
                selection,
            },
        );
    }
    Ok(())
}

fn restore_before_fetch(job: &mut Job, restore: Step) {
    let position = job
        .steps
        .iter()
        .position(|step| {
            step.name
                .starts_with(crate::source_prep::FETCH_SOURCES_STEP)
        })
        .unwrap_or(job.steps.len());
    job.steps.insert(position, restore);
}

fn replace_fetch(job: &mut Job, selected: Vec<Step>) {
    let position = job.steps.iter().position(|step| {
        step.name
            .starts_with(crate::source_prep::FETCH_SOURCES_STEP)
    });
    if let Some(position) = position {
        job.steps.retain(|step| {
            !step
                .name
                .starts_with(crate::source_prep::FETCH_SOURCES_STEP)
        });
        job.steps.splice(position..position, selected);
    }
}
