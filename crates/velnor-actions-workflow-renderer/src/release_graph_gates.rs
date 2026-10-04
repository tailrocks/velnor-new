//! Exact release role inventory, conditions and artifact dependency edges.
use crate::{
    RenderError,
    release_jobs::{ReleaseJobSpec, ReleaseRole},
    release_spec::{BootstrapPlan, publish_gate_condition, reconcile_gate_condition},
};
use std::collections::BTreeMap;

pub(super) fn check_role_set(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
    bootstrap: bool,
    preparation: bool,
) -> Result<(), RenderError> {
    let mut required = vec![
        ReleaseRole::PackageAnonymous,
        ReleaseRole::PreflightForge,
        if bootstrap {
            ReleaseRole::RegistryPublishBootstrap
        } else {
            ReleaseRole::RegistryPublishOidc
        },
        ReleaseRole::ForgePublish,
        ReleaseRole::Reconcile,
    ];
    if preparation {
        required.extend([
            ReleaseRole::PreparationAnonymous,
            ReleaseRole::PreparationForge,
        ]);
    }
    required.sort();
    let mut actual = Vec::new();
    for (id, job) in jobs {
        if id != job.role.job_id() || job.display_name != job.role.job_id() {
            return Err(invalid(id, "release_producer_identity"));
        }
        actual.push(job.role);
    }
    actual.sort();
    if actual != required {
        return Err(invalid("roles", "release_role_set"));
    }
    Ok(())
}

pub(super) fn check_needs_graph(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        for need in &job.needs {
            let Some(target) = jobs.get(need) else {
                return Err(invalid(id, "unknown_need"));
            };
            if need == id {
                return Err(invalid(id, "self_need"));
            }
            if target.role.rank() >= job.role.rank() {
                return Err(invalid(id, "backward_need"));
            }
        }
    }
    Ok(())
}

pub(super) fn check_role_conditions(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
    repository: &str,
    bootstrap: &BootstrapPlan,
    branches: &[String],
) -> Result<(), RenderError> {
    let [branch] = branches else {
        return Err(invalid("branch", "ambiguous_release_branch"));
    };
    let gate = publish_gate_condition(repository, bootstrap, branch);
    let reconcile = reconcile_gate_condition(repository, bootstrap, branch);
    for (id, job) in jobs {
        let expected = match job.role {
            ReleaseRole::RegistryPublishOidc
            | ReleaseRole::RegistryPublishBootstrap
            | ReleaseRole::ForgePublish
            | ReleaseRole::PreparationForge => Some(gate.as_str()),
            ReleaseRole::Reconcile => Some(reconcile.as_str()),
            _ => None,
        };
        if job.condition.as_deref() != expected {
            return Err(invalid(id, "release_role_condition"));
        }
    }
    Ok(())
}

pub(super) fn check_publish_needs(
    jobs: &BTreeMap<String, ReleaseJobSpec>,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        let expected: &[&str] = match job.role {
            ReleaseRole::PackagePreparedAnonymous => &["release-source-snapshot"],
            ReleaseRole::SourceSnapshotForge
            | ReleaseRole::PackageAnonymous
            | ReleaseRole::PreparationAnonymous => &[],
            ReleaseRole::PreflightForge => &["release-package"],
            ReleaseRole::RegistryPublishOidc | ReleaseRole::RegistryPublishBootstrap => {
                &["release-package", "release-preflight"]
            }
            ReleaseRole::ForgePublish => &[
                "release-package",
                "release-preflight",
                "release-registry-publish",
            ],
            ReleaseRole::Reconcile => &[
                "release-package",
                "release-preflight",
                "release-registry-publish",
                "release-forge-publish",
            ],
            ReleaseRole::PreparationForge => &["release-preparation-source"],
        };
        if !job
            .needs
            .iter()
            .map(String::as_str)
            .eq(expected.iter().copied())
        {
            return Err(invalid(id, "release_exact_needs"));
        }
    }
    Ok(())
}
fn invalid(id: &str, reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("{reason}:{id}"))
}
