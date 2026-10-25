//! Identity for cache data owned by one generated job.

use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::workflow::plan::{MatrixEntry, Plan};

use super::{QualificationCacheLayer, identity_commitment, runtime_requirements};

/// A stable record for one matrix member assigned to a generated job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheOwnerMemberRef {
    /// Stable matrix key.
    pub matrix_key: String,
    /// Registered detector ID.
    pub stack_id: String,
    /// Stable logical task-group ID.
    pub task_id: String,
    /// Complete generated task digest.
    pub task_digest: String,
    /// Complete generated input digest.
    pub input_digest: String,
}

/// Plan data for the cache owner check in directives and receipts.
#[derive(Debug, Clone)]
pub(crate) struct QualificationCacheOwner {
    pub(crate) owner_id: String,
    pub(crate) platform_id: String,
    pub(crate) member_digest: String,
    pub(crate) members: Vec<QualificationCacheOwnerMemberRef>,
    pub(crate) entries: Vec<MatrixEntry>,
}

#[derive(Serialize)]
struct OwnerPreimage<'a> {
    domain: &'static str,
    owner_id: &'a str,
    platform_id: &'a str,
    members: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct LayerPreimage<'a> {
    domain: &'static str,
    owner_id: &'a str,
    platform_id: &'a str,
    layer: QualificationCacheLayer,
    member_layers: Vec<(&'a str, String)>,
}

#[derive(Serialize)]
struct OwnerDriverPreimage<'a> {
    domain: &'static str,
    owner_id: &'a str,
    members: Vec<(String, String)>,
}

/// Build one cache owner from all plan entries for the specified job.
pub(crate) fn owner_for_plan(
    plan: &Plan,
    owner_id: &str,
) -> Result<QualificationCacheOwner, ContractError> {
    crate::workflow::validate_job_id(owner_id)?;
    let mut entries = plan
        .matrix
        .include
        .iter()
        .filter(|entry| entry.job_id == owner_id)
        .cloned()
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.matrix_key.cmp(&right.matrix_key));
    if entries.is_empty() {
        return Err(invalid("owner_has_no_matrix_members"));
    }
    let platform = &entries[0].planned_platform;
    let platform_id = platform.platform_id.clone();
    let mut members = Vec::with_capacity(entries.len());
    let mut member_preimages = Vec::with_capacity(entries.len());
    for entry in &entries {
        if entry.job_id != owner_id || entry.planned_platform != *platform {
            return Err(invalid("owner_members_have_incompatible_platforms"));
        }
        if members
            .last()
            .is_some_and(|prior: &QualificationCacheOwnerMemberRef| {
                prior.matrix_key == entry.matrix_key
            })
        {
            return Err(invalid("owner_member_duplicate"));
        }
        members.push(QualificationCacheOwnerMemberRef {
            matrix_key: entry.matrix_key.clone(),
            stack_id: entry.stack_id.clone(),
            task_id: entry.task_id.clone(),
            task_digest: entry.task_digest.clone(),
            input_digest: entry.input_digest.clone(),
        });
        let mut entry_value = serde_json::to_value(entry)
            .map_err(|_| invalid("owner_matrix_member_serialize_failed"))?;
        let object = entry_value
            .as_object_mut()
            .ok_or_else(|| invalid("owner_matrix_member_not_object"))?;
        object.remove("report_id");
        object.remove("artifact_id");
        object.insert(
            "adapter_metadata".to_owned(),
            super::canonical_adapter_metadata(plan, &entry.adapter_metadata)?,
        );
        let obligations = plan
            .obligations
            .iter()
            .filter(|obligation| obligation.job_id == owner_id)
            .collect::<Vec<_>>();
        member_preimages.push(serde_json::json!({
            "entry": entry_value,
            "owner_obligations": obligations,
        }));
    }
    let preimage = OwnerPreimage {
        domain: "velnor-qualification-cache-owner-members-v4",
        owner_id,
        platform_id: &platform_id,
        members: member_preimages,
    };
    let member_digest = digest_b3(&canonical_json_bytes(&preimage)?);
    Ok(QualificationCacheOwner {
        owner_id: owner_id.to_owned(),
        platform_id,
        member_digest,
        members,
        entries,
    })
}

/// Make the cache layer identity depend on its owner and the full sorted member set.
pub(crate) fn owner_layer_identity(
    plan: &Plan,
    owner: &QualificationCacheOwner,
    layer: QualificationCacheLayer,
) -> Result<String, ContractError> {
    let mut member_layers = Vec::with_capacity(owner.entries.len());
    let mut runtime: Option<
        Option<super::super::runtime::QualificationRuntimeIdentityRequirements>,
    > = None;
    for entry in &owner.entries {
        let requirements = runtime_requirements(plan, entry)?;
        if runtime
            .as_ref()
            .is_some_and(|prior| !same_runtime_pins(prior.as_ref(), requirements.as_ref()))
        {
            return Err(invalid("owner_members_have_incompatible_runtime_pins"));
        }
        if runtime.is_none() {
            runtime = Some(requirements);
        }
        member_layers.push((
            entry.matrix_key.as_str(),
            identity_commitment(plan, entry, layer)?,
        ));
    }
    let preimage = LayerPreimage {
        domain: "velnor-qualification-cache-owner-layer-v4",
        owner_id: &owner.owner_id,
        platform_id: &owner.platform_id,
        layer,
        member_layers,
    };
    Ok(digest_b3(&canonical_json_bytes(&preimage)?))
}

pub(crate) fn owner_runtime_requirements(
    plan: &Plan,
    owner: &QualificationCacheOwner,
) -> Result<Option<super::super::runtime::QualificationRuntimeIdentityRequirements>, ContractError>
{
    let Some(first) = owner.entries.first() else {
        return Err(invalid("owner_has_no_matrix_members"));
    };
    let Some(mut requirements) = runtime_requirements(plan, first)? else {
        for entry in owner.entries.iter().skip(1) {
            if runtime_requirements(plan, entry)?.is_some() {
                return Err(invalid("owner_members_have_incompatible_runtime_pins"));
            }
        }
        return Ok(None);
    };
    let mut driver_members = vec![(first.matrix_key.clone(), requirements.driver_id.clone())];
    for entry in owner.entries.iter().skip(1) {
        let Some(member_requirements) = runtime_requirements(plan, entry)? else {
            return Err(invalid("owner_members_have_incompatible_runtime_pins"));
        };
        if !same_runtime_pins(Some(&requirements), Some(&member_requirements)) {
            return Err(invalid("owner_members_have_incompatible_runtime_pins"));
        }
        driver_members.push((entry.matrix_key.clone(), member_requirements.driver_id));
    }
    if driver_members.len() > 1 {
        driver_members.sort_by(|left, right| left.0.cmp(&right.0));
        requirements.driver_id = digest_b3(&canonical_json_bytes(&OwnerDriverPreimage {
            domain: "velnor-qualification-cache-owner-driver-v4",
            owner_id: &owner.owner_id,
            members: driver_members,
        })?);
    }
    Ok(Some(requirements))
}

fn same_runtime_pins(
    left: Option<&super::super::runtime::QualificationRuntimeIdentityRequirements>,
    right: Option<&super::super::runtime::QualificationRuntimeIdentityRequirements>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            let mut normalized_right = right.clone();
            normalized_right.driver_id.clone_from(&left.driver_id);
            left == &normalized_right
        }
        _ => false,
    }
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("qualification.cache_owner", reason)
}
