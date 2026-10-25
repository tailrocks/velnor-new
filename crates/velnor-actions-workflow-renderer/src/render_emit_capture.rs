//! Test-feature-only census of source-bound reusable workflow children.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{Job, WorkflowIr};

#[path = "render_emit_capture_metrics.rs"]
mod metrics;

use metrics::measure_parent_candidate;

use crate::{
    RenderError, document,
    lane_share::{LaneShare, job_workflow_extract},
    render::{RenderContext, WORKFLOW_PATH, test_render_capture},
    workflow_routing::{WorkflowRoutingPolicy, parse_authority_document},
    yaml::Yaml,
};

type PreparedCaptureJobs = (
    BTreeMap<String, job_workflow_extract::PreparedJob>,
    BTreeMap<String, job_workflow_extract::CaptureRunnerMetadata>,
);

struct CaptureChildMetadata<'a> {
    source_ids: &'a BTreeSet<String>,
    runner_metadata: &'a BTreeMap<String, job_workflow_extract::CaptureRunnerMetadata>,
}

pub(super) fn begin_source_job_workflow_attempt() {
    test_render_capture::begin_source_job_workflow_attempt();
}

pub(super) fn observe_source_job_workflows(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    parent: &Yaml,
    mbx_jobs: &BTreeSet<String>,
    routing: &WorkflowRoutingPolicy,
) {
    let Some(source_ids) = test_render_capture::source_job_workflow_ids() else {
        return;
    };
    let result =
        capture_source_job_workflows(ir, jobs, ctx, parent, mbx_jobs, routing, &source_ids)
            .map_err(|error| error.to_string());
    test_render_capture::record_source_job_workflow_capture(result);
}

fn capture_source_job_workflows(
    ir: &WorkflowIr,
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
    parent: &Yaml,
    mbx_jobs: &BTreeSet<String>,
    routing: &WorkflowRoutingPolicy,
    source_ids: &BTreeSet<String>,
) -> Result<test_render_capture::SourceJobWorkflowCapture, RenderError> {
    job_workflow_extract::validate_capture_source_ids(source_ids)?;
    let (prepared, runner_metadata) = prepare_capture_jobs(jobs, source_ids)?;
    let child_metadata = CaptureChildMetadata {
        source_ids,
        runner_metadata: &runner_metadata,
    };
    let relocations = render_capture_children(
        ir,
        &prepared,
        ctx,
        parent,
        mbx_jobs,
        routing,
        &child_metadata,
    )?;
    measure_source_job_capture(parent, ctx, source_ids, &relocations, &runner_metadata)
}

fn prepare_capture_jobs(
    jobs: &BTreeMap<String, Job>,
    source_ids: &BTreeSet<String>,
) -> Result<PreparedCaptureJobs, RenderError> {
    let mut prepared = BTreeMap::new();
    let mut runner_metadata = BTreeMap::new();
    for source_id in source_ids {
        let source = jobs
            .get(source_id)
            .ok_or_else(|| invalid_capture(source_id, "typed_rust_source_job_missing"))?;
        let runner = job_workflow_extract::capture_runner_metadata(source_id, source)?;
        let child = job_workflow_extract::prepare_job_for_capture(source_id, source, source_ids)?;
        runner_metadata.insert(source_id.clone(), runner);
        prepared.insert(source_id.clone(), child);
    }
    Ok((prepared, runner_metadata))
}

fn render_capture_children(
    ir: &WorkflowIr,
    prepared: &BTreeMap<String, job_workflow_extract::PreparedJob>,
    ctx: &RenderContext,
    parent: &Yaml,
    mbx_jobs: &BTreeSet<String>,
    routing: &WorkflowRoutingPolicy,
    metadata: &CaptureChildMetadata<'_>,
) -> Result<Vec<job_workflow_extract::JobWorkflowRelocation>, RenderError> {
    let CaptureChildMetadata {
        source_ids,
        runner_metadata,
    } = metadata;
    let mut relocations = Vec::with_capacity(prepared.len());
    for (source_id, prepared_job) in prepared {
        let single_job = BTreeMap::from([(source_id.clone(), prepared_job.clone())]);
        let shared = empty_lane_share(single_job);
        let mut child = document::render_job_workflow_relocations_for_capture(
            ir, &shared, ctx, mbx_jobs, routing,
        )
        .map_err(|error| invalid_capture(source_id, &format!("child_lowering:{error}")))?;
        relocations.append(&mut child);
    }
    if relocations.len() != source_ids.len() {
        return Err(invalid_capture("capture", "source_child_count_mismatch"));
    }
    let parent_file = render_parent_capture(parent, ctx)?;
    let parent_document = parse_authority_document(&parent_file)?;
    for relocation in &relocations {
        let source_id = &relocation.prepared.source_id;
        job_workflow_extract::validate_finalized_caller(
            source_id,
            &relocation.prepared.source_job,
            parent,
            &relocation.expected_caller,
        )?;
        let runner = runner_metadata.get(source_id).ok_or_else(|| {
            invalid_capture(&relocation.prepared.source_id, "runner_metadata_missing")
        })?;
        document::validate_routed_child_projection(
            &relocation.expected_callee_file,
            &relocation.prepared.source_id,
            &parent_document,
            &runner.final_runner_labels,
        )?;
        if job_workflow_extract::rendered_child_has_unbound_needs(&relocation.expected_callee_file)?
        {
            return Err(invalid_capture(
                &relocation.prepared.source_id,
                "serialized_child_has_unbound_needs",
            ));
        }
    }
    Ok(relocations)
}

fn render_parent_capture(
    parent: &Yaml,
    ctx: &RenderContext,
) -> Result<crate::tree::RenderedFile, RenderError> {
    Ok(crate::tree::RenderedFile {
        path: WORKFLOW_PATH.to_owned(),
        bytes: crate::render::fallback::render_marked_workflow(
            WORKFLOW_PATH,
            parent,
            &ctx.generator_version,
        )?,
    })
}

fn measure_source_job_capture(
    parent: &Yaml,
    ctx: &RenderContext,
    source_ids: &BTreeSet<String>,
    relocations: &[job_workflow_extract::JobWorkflowRelocation],
    runner_metadata: &BTreeMap<String, job_workflow_extract::CaptureRunnerMetadata>,
) -> Result<test_render_capture::SourceJobWorkflowCapture, RenderError> {
    let parent_metrics = measure_parent_candidate(parent, ctx, source_ids, relocations)?;
    let target_count = relocations
        .iter()
        .map(|relocation| relocation.prepared.callee_path.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    if target_count != source_ids.len() {
        return Err(invalid_capture("capture", "callee_target_not_unique"));
    }
    let records = relocations
        .iter()
        .map(|relocation| {
            let runner = runner_metadata
                .get(&relocation.prepared.source_id)
                .ok_or_else(|| {
                    invalid_capture(&relocation.prepared.source_id, "runner_metadata_missing")
                })?;
            capture_record(relocation, runner)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(test_render_capture::SourceJobWorkflowCapture {
        validation: "source_bound_extracted_children_passed",
        source_job_ids: source_ids.iter().cloned().collect(),
        original_parent_bytes: parent_metrics.original_bytes,
        diagnostic_parent_bytes: parent_metrics.diagnostic_bytes,
        parent_bytes_saved: parent_metrics.bytes_saved,
        diagnostic_parent_sha256: parent_metrics.digest,
        extracted_unique_callee_count: target_count,
        extracted_max_call_depth: 1,
        parent_direct_workflow_targets: parent_metrics.direct_targets,
        unmeasured_parent_workflow_targets: parent_metrics.unmeasured_targets,
        full_graph_complete: false,
        full_graph_unique_callee_count: None,
        full_graph_max_call_depth: None,
        exceptions: Vec::new(),
        records,
    })
}

fn empty_lane_share(
    job_workflows: BTreeMap<String, job_workflow_extract::PreparedJob>,
) -> LaneShare {
    LaneShare {
        jobs: BTreeMap::new(),
        job_workflows,
        calls: BTreeMap::new(),
        checkouts: BTreeMap::new(),
        env_steps: BTreeMap::new(),
        runtime_preludes: BTreeMap::new(),
        prefixes: BTreeMap::new(),
        preludes: BTreeMap::new(),
        postludes: BTreeMap::new(),
        files: Vec::new(),
    }
}

fn replace_parent_callers(
    parent: &Yaml,
    relocations: &[job_workflow_extract::JobWorkflowRelocation],
) -> Result<Yaml, RenderError> {
    let mut parent = parent.clone();
    let Yaml::Map(fields) = &mut parent else {
        return Err(invalid_capture("parent", "workflow_not_map"));
    };
    let Some((_, Yaml::Map(jobs))) = fields.iter_mut().find(|(key, _)| key == "jobs") else {
        return Err(invalid_capture("parent", "workflow_jobs_missing"));
    };
    for relocation in relocations {
        let source_id = &relocation.prepared.source_id;
        let mut matches = jobs.iter_mut().filter(|(job_id, _)| job_id == source_id);
        let Some((_, actual)) = matches.next() else {
            return Err(invalid_capture(source_id, "parent_source_job_missing"));
        };
        if matches.next().is_some() {
            return Err(invalid_capture(source_id, "duplicate_parent_source_job"));
        }
        *actual = relocation.expected_caller.clone();
    }
    Ok(parent)
}

fn validate_parent_replacement_scope(
    original: &Yaml,
    diagnostic: &Yaml,
    source_ids: &BTreeSet<String>,
) -> Result<(), RenderError> {
    let Yaml::Map(original_fields) = original else {
        return Err(invalid_capture("parent", "original_workflow_not_map"));
    };
    let Yaml::Map(diagnostic_fields) = diagnostic else {
        return Err(invalid_capture("parent", "diagnostic_workflow_not_map"));
    };
    if original_fields.len() != diagnostic_fields.len() {
        return Err(invalid_capture("parent", "workflow_fields_changed"));
    }
    for ((original_key, original_value), (diagnostic_key, diagnostic_value)) in
        original_fields.iter().zip(diagnostic_fields)
    {
        if original_key != diagnostic_key {
            return Err(invalid_capture("parent", "workflow_field_order_changed"));
        }
        if original_key != "jobs" {
            if original_value != diagnostic_value {
                return Err(invalid_capture("parent", "non_job_workflow_field_changed"));
            }
            continue;
        }
        let (Yaml::Map(original_jobs), Yaml::Map(diagnostic_jobs)) =
            (original_value, diagnostic_value)
        else {
            return Err(invalid_capture("parent", "workflow_jobs_not_map"));
        };
        if original_jobs.len() != diagnostic_jobs.len() {
            return Err(invalid_capture("parent", "workflow_job_count_changed"));
        }
        for ((original_id, original_job), (diagnostic_id, diagnostic_job)) in
            original_jobs.iter().zip(diagnostic_jobs)
        {
            if original_id != diagnostic_id
                || (!source_ids.contains(original_id) && original_job != diagnostic_job)
            {
                return Err(invalid_capture(
                    original_id,
                    "non_source_parent_job_changed",
                ));
            }
        }
        if !original_jobs
            .iter()
            .any(|(id, _)| id == velnor_actions_contract::workflow::REQUIRED_JOB_ID)
            || source_ids.contains(velnor_actions_contract::workflow::REQUIRED_JOB_ID)
        {
            return Err(invalid_capture("parent", "required_job_not_preserved"));
        }
    }
    Ok(())
}

fn capture_record(
    relocation: &job_workflow_extract::JobWorkflowRelocation,
    runner: &job_workflow_extract::CaptureRunnerMetadata,
) -> Result<test_render_capture::SourceJobWorkflowRecord, RenderError> {
    let source_id = &relocation.prepared.source_id;
    let (group_id, lane) = source_group_lane(source_id)?;
    let child = &relocation.expected_callee_file;
    let digest = Sha256::digest(child.bytes.as_bytes());
    let caller_inputs =
        job_workflow_extract::source_bound_caller_inputs(&relocation.prepared.source_job)
            .into_iter()
            .map(|input| test_render_capture::SourceJobWorkflowInput {
                name: input.name.to_owned(),
                expression: input.expression.to_owned(),
            })
            .collect();
    Ok(test_render_capture::SourceJobWorkflowRecord {
        source_id: source_id.clone(),
        group_id,
        lane,
        callee_path: child.path.clone(),
        runner_class: runner.runner_class.to_owned(),
        configured_runner: runner.configured_runner.clone(),
        final_runner_labels: runner.final_runner_labels.clone(),
        child_yaml: child.bytes.clone(),
        child_bytes: child.bytes.len(),
        child_sha256: digest_hex(&digest),
        caller_inputs,
    })
}

fn parent_workflow_targets(parent: &Yaml) -> Result<BTreeSet<String>, RenderError> {
    let Yaml::Map(fields) = parent else {
        return Err(invalid_capture("parent", "workflow_not_map"));
    };
    let Some((_, Yaml::Map(jobs))) = fields.iter().find(|(key, _)| key == "jobs") else {
        return Err(invalid_capture("parent", "workflow_jobs_missing"));
    };
    let mut targets = BTreeSet::new();
    for (job_id, job) in jobs {
        let Yaml::Map(fields) = job else {
            return Err(invalid_capture(job_id, "workflow_job_not_map"));
        };
        if let Some((_, uses)) = fields.iter().find(|(key, _)| key == "uses") {
            let (Yaml::Str(target) | Yaml::Quoted(target) | Yaml::Annotated { value: target, .. }) =
                uses
            else {
                return Err(invalid_capture(job_id, "workflow_call_target_not_scalar"));
            };
            targets.insert(target.clone());
        }
    }
    Ok(targets)
}

fn source_group_lane(source_id: &str) -> Result<(String, String), RenderError> {
    let (group_id, lane) = if let Some(group_id) = source_id.strip_suffix(HOSTED_SUFFIX) {
        (group_id, "hosted")
    } else if let Some(group_id) = source_id.strip_suffix(SCALE_SUFFIX) {
        (group_id, "scale_set")
    } else {
        return Err(invalid_capture(
            source_id,
            "source_id_has_no_typed_lane_suffix",
        ));
    };
    if group_id.is_empty() {
        return Err(invalid_capture(source_id, "source_id_has_empty_group"));
    }
    Ok((group_id.to_owned(), lane.to_owned()))
}

fn invalid_capture(source_id: &str, problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("source_job_workflow_capture:{source_id}:{problem}"))
}

fn digest_hex(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(*byte >> 4)]));
        output.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    output
}
