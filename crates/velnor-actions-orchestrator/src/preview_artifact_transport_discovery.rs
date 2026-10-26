// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ArtifactRecord, BTreeMap, BTreeSet, JobRecord, OrchestratorError, PayloadArtifactRef,
    ReleaseProductionRequest, ReleaseProductionStage, internal, release_payload_artifact_name,
};

pub(super) fn validate_preview_discovery_request(
    request: &ReleaseProductionRequest,
) -> Result<(), OrchestratorError> {
    let ReleaseProductionStage::Assemble { inputs } = &request.stage else {
        return Err(internal("preview_artifact_assembly_required"));
    };
    if !inputs.is_empty() {
        return Err(internal("preview_artifact_request_inputs_unexpected"));
    }
    let pairs =
        velnor_actions_contract::preview_production::expected_release_payload_pairs(&request.plan)
            .map_err(|_| internal("preview_artifact_plan"))?;
    let mut candidate = request.clone();
    candidate.stage = ReleaseProductionStage::Assemble {
        inputs: pairs
            .into_iter()
            .enumerate()
            .map(|(index, pair)| {
                Ok(PayloadArtifactRef {
                    pair,
                    artifact_id: u64::try_from(index + 1)
                        .map_err(|_| internal("preview_artifact_request"))?,
                    sha256: "0".repeat(64),
                })
            })
            .collect::<Result<Vec<_>, OrchestratorError>>()?,
    };
    candidate
        .validate()
        .map_err(|_| internal("preview_artifact_request"))
}

pub(super) fn discover_preview_inputs(
    request: &ReleaseProductionRequest,
    inventory: &BTreeMap<u64, ArtifactRecord>,
    jobs: &[JobRecord],
) -> Result<Vec<PayloadArtifactRef>, OrchestratorError> {
    let expected_pairs =
        velnor_actions_contract::preview_production::expected_release_payload_pairs(&request.plan)
            .map_err(|_| internal("preview_artifact_plan"))?;
    let mut expected_jobs = BTreeSet::new();
    let mut inputs = Vec::with_capacity(expected_pairs.len());
    let mut ids = BTreeSet::new();
    for pair in &expected_pairs {
        let job = format!("Build {} / {}", pair.product_id, pair.target);
        expected_jobs.insert(job.clone());
        let name = release_payload_artifact_name(&request.plan, pair, &request.context)
            .map_err(|_| internal("preview_artifact_name"))?;
        let matches = inventory
            .values()
            .filter(|artifact| artifact.name == name)
            .collect::<Vec<_>>();
        let [artifact] = matches.as_slice() else {
            return Err(internal("preview_artifact_payload_inventory"));
        };
        let digest = artifact
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or_else(|| internal("preview_artifact_payload_digest"))?;
        if artifact.id == 0 || !ids.insert(artifact.id) || !lower_sha256(digest) {
            return Err(internal("preview_artifact_payload_identity"));
        }
        inputs.push(PayloadArtifactRef {
            pair: pair.clone(),
            artifact_id: artifact.id,
            sha256: digest.to_owned(),
        });
    }
    let namespace = format!(
        "velnor-release-payload-r{}-a{}-",
        request.context.run_id, request.context.run_attempt
    );
    if inventory
        .values()
        .any(|artifact| artifact.name.starts_with(&namespace) && !ids.contains(&artifact.id))
    {
        return Err(internal("preview_artifact_unexpected_payload"));
    }
    let mut observed_jobs = BTreeSet::new();
    for job in jobs.iter().filter(|job| job.name.starts_with("Build ")) {
        if job.run_id != request.context.run_id
            || job.head_sha.as_deref() != Some(request.context.workflow_authority_sha.as_str())
            || job.status.as_deref() != Some("completed")
            || job.conclusion.as_deref() != Some("success")
            || !expected_jobs.contains(&job.name)
            || !observed_jobs.insert(job.name.clone())
        {
            return Err(internal("preview_artifact_payload_job_inventory"));
        }
    }
    if observed_jobs != expected_jobs {
        return Err(internal("preview_artifact_payload_job_inventory"));
    }
    Ok(inputs)
}

fn lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
