//! Compiled source-helper fixtures for release execution-gate tests.
use std::collections::BTreeMap;

use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{
    CompiledNativeExecRecipe, CompiledSourceHelper, HelperInvocation, SourceBoundHelper,
    SourceBoundOperation as Op, Step,
};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_jobs::{ReleaseRole, ReleaseWorkflowSpec};
use velnor_actions_workflow_renderer::release_tree::RELEASE_SOURCE_DIR;

use super::{SHA, bootstrap_tools, source_checkout};

#[path = "impl_renderer_release_gate_fixture_helpers.rs"]
mod helpers;
#[path = "impl_renderer_release_prepared_fixture.rs"]
mod prepared_fixture;
use helpers::helper_record;

const RUST: &str = "rust@1.98.1";
const PYTHON: &str = "python@3.14.7";
const GH: &str = "gh@2.102.0";
const RELEASE_PLZ: &str = "release-plz@0.3.169";
const VERSION: &str = "0.1.0";

fn selectors() -> Vec<String> {
    [PYTHON, RUST, GH, RELEASE_PLZ]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn scoped_selectors(operation: Op) -> Vec<String> {
    if matches!(
        operation,
        Op::ReleaseAdmissionDefaultBranch
            | Op::RustReleaseSourceSnapshot
            | Op::RustReleasePreparedPackage
    ) {
        vec![PYTHON.to_owned()]
    } else {
        selectors()
    }
}

fn recipe(
    operation: Op,
    scope: NativeCredentialScope,
    environment: &BTreeMap<String, String>,
) -> CompiledNativeExecRecipe {
    let installed = scoped_selectors(operation);
    let mut prefix = if matches!(
        operation,
        Op::ReleaseAdmissionDefaultBranch
            | Op::RustReleaseSourceSnapshot
            | Op::RustReleasePreparedPackage
    ) {
        let mut prefix = vec!["/usr/bin/env".to_owned(), "-i".to_owned()];
        prefix.extend(environment.iter().map(|(key, value)| {
            let value = if scope.allowed_keys().contains(&key.as_str()) {
                format!("${key}")
            } else {
                value.replace("${{ runner.temp }}", "$RUNNER_TEMP")
            };
            format!("{key}={value}")
        }));
        prefix
    } else {
        vec![
            "env".to_owned(),
            "-i".to_owned(),
            "/owned/mise".to_owned(),
            "exec".to_owned(),
        ]
    };
    if matches!(
        operation,
        Op::ReleaseAdmissionDefaultBranch
            | Op::RustReleaseSourceSnapshot
            | Op::RustReleasePreparedPackage
    ) {
        prefix.extend([
            "/owned/mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "exec".to_owned(),
        ]);
    }
    prefix.extend(installed.iter().cloned());
    prefix.push("--".to_owned());
    CompiledNativeExecRecipe::compiled_for_scope(prefix, environment.clone(), installed, scope)
        .expect("release fixture recipe")
}

fn helper(
    operation: Op,
    environment: BTreeMap<String, String>,
    scope: NativeCredentialScope,
) -> CompiledSourceHelper {
    let source =
        velnor_actions_contract::generated_source(VERSION, &format!("exit 0\n# {operation:?}\n"))
            .expect("release fixture source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)
        .expect("release fixture descriptor");
    let invocation =
        HelperInvocation::compiled(descriptor, Vec::new(), scoped_selectors(operation))
            .expect("release fixture invocation");
    let helper = CompiledSourceHelper::compiled(invocation, source)
        .expect("release fixture helper")
        .with_environment(environment.clone())
        .with_execution_recipe(recipe(operation, scope, &environment))
        .expect("release fixture execution recipe");
    if matches!(
        operation,
        Op::RustReleaseSourceSnapshot | Op::RustReleasePreparedPackage
    ) {
        helper
            .with_github_output()
            .expect("source snapshot output capability")
    } else {
        helper
    }
}

pub(crate) fn operation_order(role: ReleaseRole) -> Vec<Op> {
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
    if role == ReleaseRole::PackagePreparedAnonymous {
        return vec![
            Op::MiseBootstrap,
            Op::MiseToolPrepare,
            Op::RustReleasePreparedPackage,
        ];
    }
    let mut operations = vec![
        Op::MiseBootstrap,
        match role {
            ReleaseRole::PackageAnonymous => Op::RustPrepareRootLinux,
            ReleaseRole::PreparationAnonymous => Op::RustReleasePrepareTools,
            _ => Op::MiseToolPrepare,
        },
    ];
    if !matches!(
        role,
        ReleaseRole::PackageAnonymous | ReleaseRole::PreparationAnonymous
    ) {
        operations.push(Op::ReleaseAdmissionDefaultBranch);
    }
    operations.push(match role {
        ReleaseRole::SourceSnapshotForge => Op::RustReleaseSourceSnapshot,
        ReleaseRole::PackageAnonymous => Op::RustReleaseAnonymousPackage,
        ReleaseRole::PackagePreparedAnonymous => Op::RustReleasePreparedPackage,
        ReleaseRole::PreflightForge => Op::RustReleaseForgePreflight,
        ReleaseRole::RegistryPublishOidc | ReleaseRole::RegistryPublishBootstrap => {
            Op::RustRegistryArtifactProof
        }
        ReleaseRole::ForgePublish => Op::RustForgePublish,
        ReleaseRole::Reconcile => Op::RustReleaseReconcile,
        ReleaseRole::PreparationAnonymous => Op::RustReleasePrepareAnonymous,
        ReleaseRole::PreparationForge => Op::RustReleasePrepareForge,
    });
    if matches!(
        role,
        ReleaseRole::RegistryPublishOidc | ReleaseRole::RegistryPublishBootstrap
    ) {
        operations.push(Op::RustRegistryPublish);
    }
    operations
}

pub(crate) fn helper_registry(bootstrap_mode: bool) -> Vec<CompiledSourceHelper> {
    let mut records = super::bootstrap_fixture::helper_registry();
    let roles = [
        ReleaseRole::PackageAnonymous,
        ReleaseRole::PreflightForge,
        if bootstrap_mode {
            ReleaseRole::RegistryPublishBootstrap
        } else {
            ReleaseRole::RegistryPublishOidc
        },
        ReleaseRole::ForgePublish,
        ReleaseRole::Reconcile,
        ReleaseRole::PreparationAnonymous,
        ReleaseRole::PreparationForge,
    ];
    for role in roles {
        for operation in operation_order(role).into_iter().skip(1) {
            let record = helper_record(role, operation, bootstrap_mode);
            if !records
                .iter()
                .any(|item| item.invocation() == record.invocation())
            {
                records.push(record);
            }
        }
    }
    records
}

pub(crate) fn source_helper_registry(bootstrap_mode: bool) -> Vec<CompiledSourceHelper> {
    let mut records = super::bootstrap_fixture::source_tool_records();
    for operation in [
        Op::ReleaseAdmissionDefaultBranch,
        Op::RustReleaseSourceSnapshot,
    ] {
        records.push(helper_record(
            ReleaseRole::SourceSnapshotForge,
            operation,
            bootstrap_mode,
        ));
    }
    records
}
fn helper_step(
    role: ReleaseRole,
    operation: Op,
    bootstrap_mode: bool,
    name: &str,
) -> Result<Step, RenderError> {
    let record = helper_record(role, operation, bootstrap_mode);
    velnor_actions_workflow_renderer::source_helper::source_helper_step(
        name,
        &record,
        record.environment().clone(),
    )
}

fn upload(role: ReleaseRole) -> Result<Step, RenderError> {
    velnor_actions_workflow_renderer::release_artifact_channels::upload_step(role)
}

fn job_steps(role: ReleaseRole, bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    if role == ReleaseRole::SourceSnapshotForge {
        return source_snapshot_steps(bootstrap_mode);
    }
    if role == ReleaseRole::PackagePreparedAnonymous {
        return prepared_fixture::steps(bootstrap_mode);
    }
    let mut steps = Vec::new();
    if matches!(
        role,
        ReleaseRole::PackageAnonymous | ReleaseRole::PreparationAnonymous
    ) {
        steps.push(source_checkout(SHA, Some("false"))?);
    }
    steps.push(mise_setup_step(
        &bootstrap_tools(),
        velnor_actions_contract::ToolCacheDomain::Full,
        super::LABEL,
    )?);
    for operation in operation_order(role).into_iter().skip(1) {
        let name = match operation {
            Op::ReleaseAdmissionDefaultBranch => "Admit protected CI candidate",
            Op::RustReleaseAnonymousPackage => "Create anonymous release package",
            Op::RustReleasePrepareAnonymous => "Prepare anonymous release",
            Op::RustReleaseForgePreflight => "Prove immutable release package",
            Op::RustReleaseSourceSnapshot => "Snapshot approved source",
            Op::RustReleasePreparedPackage => "Prepare anonymous package",
            Op::RustRegistryArtifactProof => "Prove registry artifact",
            Op::RustRegistryPublish => "Publish registry release",
            Op::RustForgePublish => "Publish forge release",
            Op::RustReleaseReconcile => "Reconcile release receipt",
            Op::RustPrepareRootLinux | Op::MiseToolPrepare | Op::RustReleasePrepareTools => {
                "Prepare release tools"
            }
            Op::RustReleasePrepareForge => "Prepare forge release",
            Op::MiseBootstrap => unreachable!("bootstrap is emitted by setup"),
            _ => unreachable!("release fixture operation is not in the release pipeline"),
        };
        steps.push(helper_step(role, operation, bootstrap_mode, name)?);
    }
    steps.push(upload(role)?);
    Ok(steps)
}

fn source_snapshot_steps(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    let mut steps = Vec::new();
    for record in super::bootstrap_fixture::source_tool_records() {
        steps.push(
            velnor_actions_workflow_renderer::source_helper::source_helper_step(
                "Prepare qualified source tools",
                &record,
                record.environment().clone(),
            )?,
        );
    }
    let role = ReleaseRole::SourceSnapshotForge;
    steps.push(helper_step(
        role,
        Op::ReleaseAdmissionDefaultBranch,
        bootstrap_mode,
        "Admit protected CI candidate",
    )?);
    let mut snapshot = helper_step(
        role,
        Op::RustReleaseSourceSnapshot,
        bootstrap_mode,
        "Snapshot approved source",
    )?;
    snapshot.id = Some(
        velnor_actions_contract::StepId::new("release-source-snapshot")
            .map_err(RenderError::Contract)?,
    );
    steps.push(snapshot);
    steps.push(upload(role)?);
    Ok(steps)
}

pub(crate) fn preparation(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    job_steps(ReleaseRole::PreparationForge, bootstrap_mode)
}

pub(crate) fn preparation_source(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    job_steps(ReleaseRole::PreparationAnonymous, bootstrap_mode)
}

pub(crate) fn preflight(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    job_steps(ReleaseRole::PreflightForge, bootstrap_mode)
}

pub(crate) fn package(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    job_steps(ReleaseRole::PackageAnonymous, bootstrap_mode)
}

pub(crate) fn source_snapshot(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    job_steps(ReleaseRole::SourceSnapshotForge, bootstrap_mode)
}

pub(crate) fn prepared_package(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    prepared_fixture::steps(bootstrap_mode)
}

pub(crate) fn prepared_helper_registry(bootstrap_mode: bool) -> Vec<CompiledSourceHelper> {
    prepared_fixture::helper_registry(bootstrap_mode)
}

pub(crate) fn publish(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    let role = if bootstrap_mode {
        ReleaseRole::RegistryPublishBootstrap
    } else {
        ReleaseRole::RegistryPublishOidc
    };
    job_steps(role, bootstrap_mode)
}

pub(crate) fn forge(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    job_steps(ReleaseRole::ForgePublish, bootstrap_mode)
}

pub(crate) fn reconcile(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    job_steps(ReleaseRole::Reconcile, bootstrap_mode)
}

pub(crate) fn conditioned(
    workflow: ReleaseWorkflowSpec,
    job_id: &str,
    step_name: &str,
) -> ReleaseWorkflowSpec {
    let mut steps = super::job_steps(&workflow, job_id).expect("release fixture job");
    steps
        .iter_mut()
        .find(|step| step.name == step_name)
        .expect("release fixture step")
        .condition = Some("false".to_owned());
    super::with_steps(workflow, job_id, steps).expect("release fixture job")
}

pub(crate) fn add_raw_shell(
    workflow: ReleaseWorkflowSpec,
    job_id: &str,
    name: &str,
) -> ReleaseWorkflowSpec {
    let mut steps = super::job_steps(&workflow, job_id).expect("release fixture job");
    steps.insert(steps.len().saturating_sub(1), super::raw_shell(name));
    super::with_steps(workflow, job_id, steps).expect("release fixture job")
}

pub(crate) fn add_raw_checkout(
    workflow: ReleaseWorkflowSpec,
    job_id: &str,
) -> Result<ReleaseWorkflowSpec, RenderError> {
    let mut steps = super::job_steps(&workflow, job_id).expect("release fixture job");
    steps.insert(0, source_checkout(SHA, Some("false"))?);
    Ok(super::with_steps(workflow, job_id, steps).expect("release fixture job"))
}

pub(crate) fn source_dir() -> &'static str {
    RELEASE_SOURCE_DIR
}
