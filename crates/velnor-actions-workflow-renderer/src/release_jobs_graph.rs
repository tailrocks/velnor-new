// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, ReleaseJobSpec, ReleasePlan, ReleaseRole, RenderError, publish_gate_condition,
    validate_job_id,
};

pub(super) fn check_role_set(jobs: &BTreeMap<String, ReleaseJobSpec>) -> Result<(), RenderError> {
    let mut roles: Vec<ReleaseRole> = Vec::with_capacity(jobs.len());
    for (id, job) in jobs {
        validate_job_id(id).map_err(RenderError::Contract)?;
        roles.push(job.role);
    }
    roles.sort();
    let mut required = vec![
        ReleaseRole::Preparation,
        ReleaseRole::Preflight,
        ReleaseRole::PublishOidc,
        ReleaseRole::Reconcile,
    ];
    required.sort();
    let mut with_bootstrap = vec![
        ReleaseRole::Preparation,
        ReleaseRole::Preflight,
        ReleaseRole::PublishBootstrap,
        ReleaseRole::Reconcile,
    ];
    with_bootstrap.sort();
    let preparation = jobs
        .values()
        .find(|job| job.role == ReleaseRole::Preparation);
    let has_writer = jobs.values().any(|job| job.role == ReleaseRole::PrCreate);
    if has_writer {
        required.push(ReleaseRole::PrCreate);
        with_bootstrap.push(ReleaseRole::PrCreate);
        required.sort();
        with_bootstrap.sort();
    }
    if preparation.is_none_or(|job| job.outputs.is_empty() == has_writer)
        || (roles != required && roles != with_bootstrap)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_role_set:{}",
            roles.len()
        )));
    }
    Ok(())
}

pub(super) fn check_needs_graph(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        for need in &job.needs {
            let Some(target) = jobs.get(need) else {
                return Err(RenderError::InvalidWorkflow(format!(
                    "unknown_need:{id}:{need}"
                )));
            };
            if need == id {
                return Err(RenderError::InvalidWorkflow(format!("self_need:{id}")));
            }
            if target.role.rank() >= job.role.rank() {
                return Err(RenderError::InvalidWorkflow(format!(
                    "backward_need:{id}:{need}"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn check_role_conditions(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
    repository: &str,
    branch: &str,
    plan: &ReleasePlan,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        match job.role {
            ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap => {
                let gate = publish_gate_condition(
                    repository,
                    branch,
                    plan,
                    job.role == ReleaseRole::PublishOidc,
                );
                if job.condition.as_deref() != Some(gate.as_str()) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "publish_gate_mismatch:{id}"
                    )));
                }
            }
            ReleaseRole::Reconcile => {
                let publisher = if jobs
                    .values()
                    .any(|candidate| candidate.role == ReleaseRole::PublishBootstrap)
                {
                    "release-publish-bootstrap"
                } else {
                    "release-publish"
                };
                let condition = format!("always() && needs.{publisher}.result != 'skipped'");
                if job.condition.as_deref() != Some(condition.as_str()) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "reconcile_condition:{id}"
                    )));
                }
            }
            ReleaseRole::Preparation => {
                let expected = format!(
                    "github.repository == '{repository}' && github.event_name == 'push' && github.ref == 'refs/heads/{branch}' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'"
                );
                if job.condition.as_deref() != Some(expected.as_str())
                    || job.needs != ["release-preflight"]
                {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "preparation_gate_mismatch:{id}"
                    )));
                }
            }
            ReleaseRole::PrCreate => {
                let expected = "needs.release-preparation.result == 'success' && needs.release-preparation.outputs.changed == 'true'";
                if job.condition.as_deref() != Some(expected)
                    || job.needs != ["release-preparation"]
                {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "release_pr_create_gate:{id}"
                    )));
                }
            }
            ReleaseRole::Preflight => {}
        }
    }
    Ok(())
}

pub(super) fn check_publish_needs(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
) -> Result<(), RenderError> {
    let id_for = |role: ReleaseRole| {
        jobs.iter()
            .find_map(|(id, job)| (job.role == role).then_some(id))
    };
    let Some(preflight) = id_for(ReleaseRole::Preflight) else {
        return Err(RenderError::InvalidWorkflow(
            "release_role_set:preflight".to_owned(),
        ));
    };
    let publishers: Vec<&String> = jobs
        .iter()
        .filter(|(_, job)| {
            matches!(
                job.role,
                ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap
            )
        })
        .map(|(id, _)| id)
        .collect();
    for publisher in &publishers {
        let gated = jobs
            .get(*publisher)
            .is_some_and(|job| job.needs.contains(preflight));
        if !gated {
            return Err(RenderError::InvalidWorkflow(format!(
                "publish_without_preflight:{publisher}"
            )));
        }
    }
    let Some(reconcile) = id_for(ReleaseRole::Reconcile) else {
        return Err(RenderError::InvalidWorkflow(
            "release_role_set:reconcile".to_owned(),
        ));
    };
    let reconciled = jobs.get(reconcile).is_some_and(|job| {
        publishers
            .iter()
            .all(|publisher| job.needs.contains(*publisher))
    });
    if !reconciled {
        return Err(RenderError::InvalidWorkflow(format!(
            "reconcile_without_publish:{reconcile}"
        )));
    }
    Ok(())
}
