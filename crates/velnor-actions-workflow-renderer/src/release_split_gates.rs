//! Exact source-helper execution order and independently bound artifact authority.
use crate::{
    RenderError,
    release_jobs::{ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec},
};
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope as Scope;
use velnor_actions_contract::{HelperInvocation, SourceBoundOperation as Op, StepKind};

pub(super) fn check_job(
    spec: &ReleaseWorkflowSpec,
    id: &str,
    job: &ReleaseJobSpec,
) -> Result<(), RenderError> {
    if job.outputs != crate::release_artifact_channels::job_outputs(job.role)? {
        return Err(invalid(id, "release_output_authority"));
    }
    velnor_actions_contract::workflow::outputs::validate_job_outputs(&job.outputs, &job.steps)
        .map_err(RenderError::Contract)?;
    let anonymous = matches!(
        job.role,
        ReleaseRole::PackageAnonymous | ReleaseRole::PreparationAnonymous
    );
    let expected = expected_operations(job.role);
    let mut operations = Vec::new();
    let mut checkouts = 0;
    let mut downloads = 0;
    for (index, step) in job.steps.iter().enumerate() {
        match &step.kind {
            StepKind::SourceBoundHelper { invocation, env } => {
                if step.condition.is_some() {
                    return Err(invalid(id, "release_helper_condition"));
                }
                check_helper(spec, id, job.role, invocation, env)?;
                operations.push(invocation.descriptor().operation());
            }
            StepKind::Action { uses, with, env }
                if anonymous && uses == &spec.bootstrap_tools.checkout_uses =>
            {
                if !operations.is_empty() || !env.is_empty() || step.condition.is_some() {
                    return Err(invalid(id, "release_checkout_order"));
                }
                crate::release_checkout_gates::check_checkout_shape(
                    id,
                    std::slice::from_ref(step),
                    job.role,
                    &spec.bootstrap.source_sha,
                    &spec.repository,
                )?;
                if with.keys().any(|key| {
                    ![
                        "repository",
                        "persist-credentials",
                        "fetch-depth",
                        "path",
                        "ref",
                    ]
                    .contains(&key.as_str())
                }) {
                    return Err(invalid(id, "release_checkout_inputs"));
                }
                checkouts += 1;
            }
            StepKind::Action { uses, .. }
                if job.role == ReleaseRole::PackagePreparedAnonymous
                    && uses == crate::steps::DOWNLOAD_ARTIFACT_USES =>
            {
                source_download::check(step, index)?;
                downloads += 1;
            }
            StepKind::Action { .. } if index + 1 == job.steps.len() => {
                crate::release_artifact_channels::check_upload(job.role, step)?;
            }
            _ => return Err(invalid(id, "release_repository_execution")),
        }
    }
    if operations != expected
        || (anonymous && checkouts != 1)
        || downloads != usize::from(job.role == ReleaseRole::PackagePreparedAnonymous)
    {
        return Err(invalid(id, "release_exact_execution"));
    }
    let Some(last) = job.steps.last() else {
        return Err(invalid(id, "release_missing_artifact"));
    };
    crate::release_artifact_channels::check_upload(job.role, last)
}

fn expected_operations(role: ReleaseRole) -> Vec<Op> {
    if role == ReleaseRole::PackagePreparedAnonymous {
        return vec![
            Op::MiseBootstrap,
            Op::MiseToolPrepare,
            Op::RustReleasePreparedPackage,
        ];
    }
    if role == ReleaseRole::SourceSnapshotForge {
        return vec![
            Op::MiseBootstrap,
            Op::MiseBootstrap,
            Op::MiseToolPrepare,
            Op::MiseToolPrepare,
            Op::ReleaseAdmissionDefaultBranch,
            Op::RustReleaseSourceSnapshot,
        ];
    }
    let mut result = vec![Op::MiseBootstrap];
    result.push(match role {
        ReleaseRole::PackageAnonymous => Op::RustPrepareRootLinux,
        ReleaseRole::PreparationAnonymous => Op::RustReleasePrepareTools,
        _ => Op::MiseToolPrepare,
    });
    if !matches!(
        role,
        ReleaseRole::PackageAnonymous | ReleaseRole::PreparationAnonymous
    ) {
        result.push(Op::ReleaseAdmissionDefaultBranch);
    }
    match role {
        ReleaseRole::SourceSnapshotForge => result.push(Op::RustReleaseSourceSnapshot),
        ReleaseRole::PackageAnonymous => result.push(Op::RustReleaseAnonymousPackage),
        ReleaseRole::PackagePreparedAnonymous => result.push(Op::RustReleasePreparedPackage),
        ReleaseRole::PreflightForge => result.push(Op::RustReleaseForgePreflight),
        ReleaseRole::RegistryPublishOidc | ReleaseRole::RegistryPublishBootstrap => {
            result.extend([Op::RustRegistryArtifactProof, Op::RustRegistryPublish]);
        }
        ReleaseRole::ForgePublish => result.push(Op::RustForgePublish),
        ReleaseRole::Reconcile => result.push(Op::RustReleaseReconcile),
        ReleaseRole::PreparationAnonymous => result.push(Op::RustReleasePrepareAnonymous),
        ReleaseRole::PreparationForge => result.push(Op::RustReleasePrepareForge),
    }
    result
}

fn check_helper(
    spec: &ReleaseWorkflowSpec,
    id: &str,
    role: ReleaseRole,
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let op = invocation.descriptor().operation();
    let record = spec
        .helper_registry
        .iter()
        .find(|record| record.invocation() == invocation && record.environment() == env)
        .ok_or_else(|| invalid(id, "release_helper_authority"))?;
    let scope = match op {
        Op::MiseBootstrap
        | Op::MiseToolPrepare
        | Op::RustPrepareRootLinux
        | Op::RustReleasePrepareTools => return Ok(()),
        Op::RustReleaseAnonymousPackage
        | Op::RustReleasePrepareAnonymous
        | Op::RustReleasePreparedPackage => Scope::Anonymous,
        Op::RustRegistryPublish if role == ReleaseRole::RegistryPublishOidc => {
            Scope::RustRegistryPublishOidc
        }
        Op::RustRegistryPublish if role == ReleaseRole::RegistryPublishBootstrap => {
            Scope::RustRegistryPublishBootstrap
        }
        Op::RustForgePublish | Op::RustReleasePrepareForge => Scope::GithubReleasePublish,
        _ => Scope::GithubReadOnly,
    };
    if record
        .execution_recipe()
        .is_none_or(|recipe| recipe.credential_scope() != scope)
    {
        return Err(invalid(id, "release_credential_scope"));
    }
    if op == Op::ReleaseAdmissionDefaultBranch {
        return check_admission(spec, id, env);
    }
    if env.get("RELEASE_RECONCILE_POLICY") != Some(&spec.reconciliation.serialized()?) {
        return Err(invalid(id, "release_policy_binding"));
    }
    if matches!(
        op,
        Op::RustReleaseAnonymousPackage | Op::RustReleasePrepareAnonymous
    ) {
        check_package_policy(spec, id, env)?;
    }
    bindings::check(role, id, env)
}

fn check_admission(
    spec: &ReleaseWorkflowSpec,
    id: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if env.get("APPROVED_REPOSITORY") != Some(&spec.repository)
        || env.get("APPROVED_SOURCE_SHA") != Some(&spec.bootstrap.source_sha)
        || env.get("APPROVED_DEFAULT_BRANCH") != spec.triggers.push_branches.first()
        || env.get("ADMISSION_EVENT_POLICY").map(String::as_str) != Some("default-branch")
        || env.get("ADMISSION_REF_KIND").map(String::as_str) != Some("branch")
    {
        return Err(invalid(id, "release_admission_binding"));
    }
    Ok(())
}
fn check_package_policy(
    spec: &ReleaseWorkflowSpec,
    id: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let packages = serde_json::to_string(&spec.bootstrap.packages)
        .map_err(|error| invalid(id, &error.to_string()))?;
    if env.get("RELEASE_EXPECTED_PACKAGES") != Some(&packages)
        || env.get("RELEASE_REPOSITORY") != Some(&spec.repository)
        || env.get("RELEASE_REGISTRY") != Some(&spec.bootstrap.registry)
        || env.get("RELEASE_RUST_TOOLCHAIN") != spec.reconciliation.tools.get("rust")
    {
        return Err(invalid(id, "release_package_binding"));
    }
    Ok(())
}
fn invalid(id: &str, reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("{reason}:{id}"))
}
#[path = "release_artifact_bindings.rs"]
mod bindings;
#[path = "release_source_download.rs"]
mod source_download;
