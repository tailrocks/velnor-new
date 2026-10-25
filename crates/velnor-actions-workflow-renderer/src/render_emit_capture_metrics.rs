//! Candidate parent size and direct-call metrics for the opt-in census.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use super::{
    invalid_capture, parent_workflow_targets, replace_parent_callers,
    validate_parent_replacement_scope,
};
use crate::{RenderError, lane_share::job_workflow_extract, render::RenderContext, yaml::Yaml};

pub(super) struct ParentMetrics {
    pub(super) original_bytes: usize,
    pub(super) diagnostic_bytes: usize,
    pub(super) bytes_saved: i64,
    pub(super) digest: String,
    pub(super) direct_targets: Vec<String>,
    pub(super) unmeasured_targets: Vec<String>,
}

pub(super) fn measure_parent_candidate(
    parent: &Yaml,
    ctx: &RenderContext,
    source_ids: &BTreeSet<String>,
    relocations: &[job_workflow_extract::JobWorkflowRelocation],
) -> Result<ParentMetrics, RenderError> {
    let caller_parent = replace_parent_callers(parent, relocations)?;
    validate_parent_replacement_scope(parent, &caller_parent, source_ids)?;
    validate_candidate_relocations(&caller_parent, relocations)?;
    let original = render_parent(parent, ctx)?;
    let diagnostic = render_parent(&caller_parent, ctx)?;
    let bytes_saved = checked_size_delta(original.len(), diagnostic.len())?;
    let direct_targets = parent_workflow_targets(&caller_parent)?;
    let extracted_targets = relocations
        .iter()
        .map(|relocation| format!("./{}", relocation.prepared.callee_path))
        .collect::<BTreeSet<_>>();
    if !extracted_targets.is_subset(&direct_targets) {
        return Err(invalid_capture(
            "capture",
            "extracted_callee_missing_from_parent",
        ));
    }
    let unmeasured_targets = direct_targets
        .difference(&extracted_targets)
        .cloned()
        .collect();
    Ok(ParentMetrics {
        original_bytes: original.len(),
        diagnostic_bytes: diagnostic.len(),
        bytes_saved,
        digest: digest_hex(&Sha256::digest(diagnostic.as_bytes())),
        direct_targets: direct_targets.into_iter().collect(),
        unmeasured_targets,
    })
}

fn validate_candidate_relocations(
    caller_parent: &Yaml,
    relocations: &[job_workflow_extract::JobWorkflowRelocation],
) -> Result<(), RenderError> {
    let child_files = relocations
        .iter()
        .map(|relocation| relocation.expected_callee_file.clone())
        .collect::<Vec<_>>();
    crate::workflow_routing::validate_relocated_job_workflows(
        caller_parent,
        &child_files,
        relocations,
    )
}

fn render_parent(parent: &Yaml, ctx: &RenderContext) -> Result<String, RenderError> {
    crate::render::fallback::render_marked_workflow(
        super::super::WORKFLOW_PATH,
        parent,
        &ctx.generator_version,
    )
}

fn checked_size_delta(original: usize, diagnostic: usize) -> Result<i64, RenderError> {
    let original = i64::try_from(original)
        .map_err(|_| invalid_capture("parent", "rendered_size_out_of_range"))?;
    let diagnostic = i64::try_from(diagnostic)
        .map_err(|_| invalid_capture("parent", "rendered_size_out_of_range"))?;
    original
        .checked_sub(diagnostic)
        .ok_or_else(|| invalid_capture("parent", "rendered_size_delta_out_of_range"))
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
