//! Credentialed acquisition ends before any repository code starts. The runner's
//! step output, not mutable on-disk metadata, binds the token-free consumer.

use super::{
    aggregate::{self, AdmittedLeaves},
    context,
    handoff::{self, PinnedAsset, TaskInput},
    leaf_manifest::{self, PreviewLeafManifest},
};
use crate::{
    OrchestratorError, internal::internal, retrieve_reports::staged_reads::read_staged_bytes_until,
};
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use velnor_actions_contract::{
    canonical_json_bytes,
    config::RustBinaryPreviewConfig,
    preview_production::{PreviewProductionRequest, PreviewProductionStage},
};
use velnor_actions_mise::CheckDeadline;

/// Source-free acquisition of the exact complete leaf union.
pub const ACQUIRE_PREVIEW_LEAVES_OP: &str = "acquire-preview-leaves-v1";
const RECORD: &str = "preview-admitted-leaves.json";
const SCHEMA: &str = "velnor.preview-admitted-leaves.v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    request: PreviewProductionRequest,
    leaves: Vec<PreviewLeafManifest>,
    inputs: Vec<TaskInput>,
}

/// Acquire authenticated inputs without opening source configuration or running
/// repository commands. The caller publishes the returned digest as a step output.
/// # Errors
/// Rejects remote, request, artifact, inventory and deadline drift.
pub fn acquire_preview_leaves() -> Result<String, OrchestratorError> {
    let deadline =
        CheckDeadline::from_start_with_cleanup_reserve(Instant::now(), Duration::from_secs(300))
            .map_err(|error| internal(&error.to_string()))?;
    deadline
        .scope(|| {
            let temp = runner_temp()?;
            let plan = super::read_plan(&temp, deadline)?;
            let identity = context::workflow_context(&plan, deadline)?;
            let request = context::from_context(plan, identity)?;
            if !matches!(request.stage, PreviewProductionStage::Assemble { .. }) {
                return Err(internal("preview_acquisition_stage"));
            }
            let admitted = aggregate::acquire(&temp, &request, deadline)?;
            let record = Admission {
                schema: SCHEMA.to_owned(),
                request,
                leaves: admitted.manifests,
                inputs: admitted.inputs,
            };
            let bytes = leaf_manifest::encode(&record)?;
            crate::exclusive_write::write_exclusive_until(
                &temp.join(RECORD),
                &bytes,
                "preview_admission",
                || handoff::checkpoint(deadline),
            )?;
            Ok(handoff::sha256(&bytes))
        })
        .map_err(|error| internal(&error.to_string()))?
}

pub(super) fn reject_credentials() -> Result<(), OrchestratorError> {
    credentials_absent(
        ["GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .any(|key| env::var_os(key).is_some()),
    )
}

fn credentials_absent(present: bool) -> Result<(), OrchestratorError> {
    if present {
        return Err(internal("preview_source_credentials_present"));
    }
    Ok(())
}

pub(super) fn load(
    temp: &Path,
    request: &PreviewProductionRequest,
    preview: &RustBinaryPreviewConfig,
    deadline: CheckDeadline,
) -> Result<AdmittedLeaves, OrchestratorError> {
    let expected = context::required_env("VELNOR_PREVIEW_ADMISSION_DIGEST")?;
    let bytes = read_staged_bytes_until(&temp.join(RECORD), handoff::MAX_NATIVE_MANIFEST, || {
        deadline
            .remaining()
            .map(|_| ())
            .map_err(|_| "preview_deadline")
    })
    .map_err(|_| internal("preview_admission_read"))?;
    let record = parse(&bytes, &expected, request)?;
    restore(record, preview, deadline)
}

fn parse(
    bytes: &[u8],
    digest: &str,
    request: &PreviewProductionRequest,
) -> Result<Admission, OrchestratorError> {
    if digest.len() != 64 || handoff::sha256(bytes) != digest {
        return Err(internal("preview_admission_digest"));
    }
    let limit = usize::try_from(handoff::MAX_NATIVE_MANIFEST)
        .map_err(|_| internal("preview_admission_limit"))?;
    let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(bytes, limit)
        .map_err(|_| internal("preview_admission_json"))?;
    let record: Admission =
        serde_json::from_value(value).map_err(|_| internal("preview_admission_shape"))?;
    if record.schema != SCHEMA
        || record.request != *request
        || canonical_json_bytes(&record).map_err(|_| internal("preview_admission_encode"))? != bytes
    {
        return Err(internal("preview_admission_request"));
    }
    Ok(record)
}

fn restore(
    record: Admission,
    preview: &RustBinaryPreviewConfig,
    deadline: CheckDeadline,
) -> Result<AdmittedLeaves, OrchestratorError> {
    let PreviewProductionStage::Assemble { inputs } = &record.request.stage else {
        return Err(internal("preview_admission_stage"));
    };
    if inputs.len() != record.leaves.len() {
        return Err(internal("preview_admission_inventory"));
    }
    let mut expected = Vec::new();
    for (reference, leaf) in inputs.iter().zip(&record.leaves) {
        let mut request = record.request.clone();
        request.stage = PreviewProductionStage::Payload {
            pair: reference.pair.clone(),
        };
        PreviewLeafManifest::parse(&leaf_manifest::encode(leaf)?, &request)?;
        leaf.validate_source(preview)?;
        for proof in &leaf.assets {
            expected.push((proof.name.clone(), reference.pair.clone(), proof.clone()));
        }
    }
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    if expected.len() != record.inputs.len() {
        return Err(internal("preview_admission_inventory"));
    }
    let mut pinned = Vec::new();
    let mut total = 0u64;
    for (input, (name, pair, proof)) in record.inputs.iter().zip(expected) {
        if input.name != name
            || input.pair != pair
            || input.sha256 != proof.sha256
            || input.size != proof.size
            || !Path::new(&input.path).is_absolute()
            || Path::new(&input.path)
                .file_name()
                .is_none_or(|base| base != name.as_str())
        {
            return Err(internal("preview_admission_input"));
        }
        total = total
            .checked_add(proof.size)
            .filter(|size| *size <= crate::package_release_artifacts::MAX_TOTAL_BYTES)
            .ok_or_else(|| internal("preview_leaf_asset_total"))?;
        pinned.push(PinnedAsset::new(Path::new(&input.path), &proof, deadline)?);
    }
    let admitted = AdmittedLeaves {
        manifests: record.leaves,
        inputs: record.inputs,
        pinned,
        manifest_bytes: 0,
        asset_bytes: total,
    };
    admitted.recheck(deadline)?;
    Ok(admitted)
}

fn runner_temp() -> Result<PathBuf, OrchestratorError> {
    let temp = PathBuf::from(context::required_env("RUNNER_TEMP")?)
        .canonicalize()
        .map_err(|_| internal("preview_runner_temp"))?;
    let root = temp.join("velnor");
    crate::exclusive_write::create_dir_no_symlink(&temp, &root)?;
    Ok(root)
}

#[cfg(test)]
#[path = "preview_production_acquisition_tests.rs"]
mod tests;
