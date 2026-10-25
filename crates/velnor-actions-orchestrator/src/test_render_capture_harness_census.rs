//! Typed Rust owner selection and external source-child capture artifacts.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;

use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_contract::{RunnerPlatform, RunsOn, Stack, runner_platform};
use velnor_actions_workflow_renderer::render::test_render_capture::{
    SourceJobWorkflowCapture, take_source_job_workflow_capture, with_source_job_workflow_capture,
};

use crate::prepare::GenerationPreparation;

pub(super) fn typed_rust_lane_job_ids(
    preparation: &GenerationPreparation,
) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let proposals = &preparation.discovery.proposals;
    let grouped = crate::crate_job_ids::group_runnable(proposals)?;
    let owners = crate::crate_job_ids::JobOwners::from_grouped(proposals, &grouped);
    let rust_groups: Vec<_> = grouped
        .keys()
        .filter(|group| group.stack == Stack::Rust)
        .collect();
    let mut source_ids = BTreeSet::new();
    for group in &rust_groups {
        let owner = owners
            .group_id(group)
            .ok_or("typed Rust group has no assigned job owner")?;
        source_ids.insert(format!("{owner}{HOSTED_SUFFIX}"));
        source_ids.insert(format!("{owner}{SCALE_SUFFIX}"));
    }
    if rust_groups.len() != 20 || source_ids.len() != 40 {
        return Err(format!(
            "typed Rust owner inventory changed: {} groups, {} lane jobs",
            rust_groups.len(),
            source_ids.len()
        )
        .into());
    }
    Ok(source_ids)
}

pub(super) fn render_with_source_capture<T>(
    source_ids: Option<&BTreeSet<String>>,
    render: impl FnOnce() -> T,
) -> (T, Option<Result<SourceJobWorkflowCapture, String>>) {
    velnor_actions_workflow_renderer::render::test_render_capture::clear();
    match source_ids {
        Some(source_ids) => with_source_job_workflow_capture(source_ids.clone(), || {
            let rendered = velnor_actions_workflow_renderer::render::test_render_capture::
                with_full_tree_capture(render);
            let children = take_source_job_workflow_capture();
            (rendered, children)
        }),
        None => (
            velnor_actions_workflow_renderer::render::test_render_capture::with_full_tree_capture(
                render,
            ),
            None,
        ),
    }
}

pub(super) fn write_source_job_workflow_census(
    output: &Path,
    expected_ids: &BTreeSet<String>,
    result: Option<Result<SourceJobWorkflowCapture, String>>,
    original_selected_bytes: usize,
) -> Result<(), Box<dyn Error>> {
    let capture = match result {
        Some(Ok(capture)) => capture,
        Some(Err(error)) => {
            std::fs::write(
                output.join("source-job-census-error.txt"),
                format!("source-bound census failed\n{error}\n"),
            )?;
            return Err(error.into());
        }
        None => {
            std::fs::write(
                output.join("source-job-census-error.txt"),
                "source-bound census did not run\n",
            )?;
            return Err("source-bound census did not run".into());
        }
    };
    if let Err(error) = validate_census(expected_ids, &capture, original_selected_bytes) {
        std::fs::write(
            output.join("source-job-census-error.txt"),
            format!("source-bound census validation failed\n{error}\n"),
        )?;
        return Err(error);
    }
    write_census_files(output, &capture)?;
    Ok(())
}

fn validate_census(
    expected_ids: &BTreeSet<String>,
    capture: &SourceJobWorkflowCapture,
    original_selected_bytes: usize,
) -> Result<(), Box<dyn Error>> {
    validate_graph_inventory(expected_ids, capture, original_selected_bytes)?;
    let mut lanes = BTreeMap::<String, BTreeSet<String>>::new();
    for record in &capture.records {
        if record.child_bytes != record.child_yaml.len()
            || record.child_bytes > 500_000
            || crate::generator::sha256_hex(record.child_yaml.as_bytes()) != record.child_sha256
            || record.callee_path != format!(".github/workflows/ci-rust-{}.yml", record.source_id)
            || !valid_runner_metadata(record)
            || !valid_caller_inputs(&record.caller_inputs)
        {
            return Err(format!(
                "source-bound child capture metadata is inconsistent: {}",
                record.source_id
            )
            .into());
        }
        let group_lanes = lanes.entry(record.group_id.clone()).or_default();
        if !group_lanes.insert(record.lane.clone()) {
            return Err(format!("duplicate lane capture for {}", record.group_id).into());
        }
    }
    if lanes.len() != 20
        || lanes
            .values()
            .any(|pair| pair.len() != 2 || !pair.contains("hosted") || !pair.contains("scale_set"))
    {
        return Err("source-bound census contains an incomplete typed Rust lane pair".into());
    }
    Ok(())
}

fn validate_graph_inventory(
    expected_ids: &BTreeSet<String>,
    capture: &SourceJobWorkflowCapture,
    original_selected_bytes: usize,
) -> Result<(), Box<dyn Error>> {
    let expected_ids: Vec<_> = expected_ids.iter().cloned().collect();
    let actual_ids: Vec<_> = capture
        .records
        .iter()
        .map(|record| record.source_id.clone())
        .collect();
    let extracted_targets: BTreeSet<_> = capture
        .records
        .iter()
        .map(|record| format!("./{}", record.callee_path))
        .collect();
    let parent_targets: BTreeSet<_> = capture
        .parent_direct_workflow_targets
        .iter()
        .cloned()
        .collect();
    let unmeasured_targets: BTreeSet<_> = capture
        .unmeasured_parent_workflow_targets
        .iter()
        .cloned()
        .collect();
    let expected_delta = i128::try_from(capture.original_parent_bytes)?
        - i128::try_from(capture.diagnostic_parent_bytes)?;
    if capture.validation != "source_bound_extracted_children_passed"
        || capture.source_job_ids != expected_ids
        || actual_ids != expected_ids
        || capture.records.len() != 40
        || capture.original_parent_bytes != original_selected_bytes
        || i128::from(capture.parent_bytes_saved) != expected_delta
        || capture.extracted_unique_callee_count != 40
        || capture.extracted_max_call_depth != 1
        || extracted_targets.len() != 40
        || !extracted_targets.is_subset(&parent_targets)
        || unmeasured_targets
            != parent_targets
                .difference(&extracted_targets)
                .cloned()
                .collect()
        || capture.parent_direct_workflow_targets.len() != parent_targets.len()
        || capture.unmeasured_parent_workflow_targets.len() != unmeasured_targets.len()
        || capture.full_graph_complete
        || capture.full_graph_unique_callee_count.is_some()
        || capture.full_graph_max_call_depth.is_some()
        || !capture.exceptions.is_empty()
    {
        return Err("source-bound census does not cover the exact 40 typed Rust lane jobs or overstates its graph scope".into());
    }
    Ok(())
}

fn valid_runner_metadata(
    record: &velnor_actions_workflow_renderer::render::test_render_capture::SourceJobWorkflowRecord,
) -> bool {
    match (
        record.lane.as_str(),
        RunsOn::parse(&record.configured_runner),
    ) {
        ("hosted", Ok(RunsOn::Hosted(label))) => {
            record.runner_class == "hosted-linux-x64"
                && runner_platform(&label) == Some(RunnerPlatform::LinuxAmd64)
                && record.final_runner_labels.as_slice() == std::slice::from_ref(&label)
        }
        ("scale_set", Ok(RunsOn::ScaleSet(selector))) => {
            record.runner_class == "configured-scale-set-linux-x64"
                && record.final_runner_labels.as_slice() == selector.labels()
        }
        _ => false,
    }
}

fn valid_caller_inputs(
    inputs: &[velnor_actions_workflow_renderer::render::test_render_capture::SourceJobWorkflowInput],
) -> bool {
    const EXPECTED: [(&str, &str); 5] = [
        ("plan_result", "${{ needs.plan.result }}"),
        (
            "qualification_campaign",
            "${{ needs.plan.outputs.qualification_campaign }}",
        ),
        (
            "run_key_attempt",
            "${{ needs.plan.outputs.plan_attempt || github.run_attempt }}",
        ),
        ("sources_key", "${{ needs.plan.outputs.sources_key }}"),
        (
            "sources_restore_keys",
            "${{ needs.plan.outputs.sources_restore_keys }}",
        ),
    ];
    inputs.len() == EXPECTED.len()
        && inputs
            .iter()
            .zip(EXPECTED)
            .all(|(input, (name, expression))| input.name == name && input.expression == expression)
}

fn write_census_files(
    output: &Path,
    capture: &SourceJobWorkflowCapture,
) -> Result<(), Box<dyn Error>> {
    let children = output.join("job-workflows");
    std::fs::create_dir_all(&children)?;
    for record in &capture.records {
        let filename = Path::new(&record.callee_path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("source-bound child path has no filename")?;
        if filename.contains('/') || filename.contains('\\') {
            return Err("source-bound child filename is not flat".into());
        }
        std::fs::write(children.join(filename), &record.child_yaml)?;
    }
    std::fs::write(
        output.join("rust-job-ids.json"),
        serde_json::to_vec_pretty(&capture.source_job_ids)?,
    )?;
    std::fs::write(
        output.join("source-job-workflow-census.json"),
        serde_json::to_vec_pretty(capture)?,
    )?;
    Ok(())
}
