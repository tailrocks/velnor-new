//! Closed pure executable production; metadata alone never grants helper authority.
use crate::{MiseSetup, RenderError};
use velnor_actions_contract::{
    CacheSnapshotDomain, CompiledSourceHelper, Job, Permissions, PureToolProducer,
    SourceBoundOperation, Step, StepKind, ToolCacheDomain, workflow::permissions::PermissionLevel,
};

#[path = "cache_snapshot_admission.rs"]
mod snapshots;

#[path = "cache_tool_report.rs"]
mod report;
pub use report::report_record;
use report::{report_step, validate_report};

/// Canonical producer sequence. Installation semantics belong to its compiled owner.
/// # Errors
/// Rejects unqualified installation operations or mismatched owned tool identity.
pub fn producer_steps(
    metadata: &PureToolProducer,
    setup: &MiseSetup,
    installation: &CompiledSourceHelper,
    before_snapshot: &CompiledSourceHelper,
    after_snapshot: &CompiledSourceHelper,
    version: &str,
) -> Result<Vec<Step>, RenderError> {
    metadata.validate().map_err(RenderError::Contract)?;
    let mut install = crate::source_helper::source_helper_step(
        "Prepare and verify executable cache",
        installation,
        installation.environment().clone(),
    )?;
    install.id = Some(metadata.installation_step.clone());
    validate_install(&install, metadata, setup)?;
    crate::tool_producer_steps::validate_tool_descriptor(
        &metadata.descriptor,
        setup,
        &install,
        std::slice::from_ref(installation),
    )?;
    let bootstrap = bootstrap_step(metadata, setup)?;
    validate_bootstrap(&bootstrap, metadata, setup)?;
    sequence(
        metadata,
        install,
        bootstrap,
        snapshots::snapshot_step(
            CacheSnapshotDomain::tool_domain(metadata.descriptor.domain),
            true,
            &metadata.before_step,
            before_snapshot,
        )?,
        snapshots::snapshot_step(
            CacheSnapshotDomain::tool_domain(metadata.descriptor.domain),
            false,
            &metadata.after_step,
            after_snapshot,
        )?,
        report_step(metadata, version)?,
    )
}

fn sequence(
    meta: &PureToolProducer,
    install: Step,
    bootstrap: Step,
    before: Step,
    after: Step,
    report: Step,
) -> Result<Vec<Step>, RenderError> {
    if meta.after_step.as_str() != "velnor-tool-after"
        || meta.restore_step.as_str()
            != CacheSnapshotDomain::tool_domain(meta.descriptor.domain).restore_id()
    {
        return Err(invalid("tool_producer_after_binding_changed"));
    }
    Ok(vec![
        crate::tool_producer_steps::tool_producer_platform_step(&meta.descriptor)?,
        crate::tool_producer_steps::tool_producer_restore_step(meta)?,
        before,
        bootstrap,
        install,
        after,
        crate::tool_producer_steps::tool_producer_save_step(meta)?,
        publication_step(meta)?,
        report,
    ])
}

fn publication_step(meta: &PureToolProducer) -> Result<Step, RenderError> {
    let save = crate::tool_producer_steps::tool_producer_save_step(meta)?;
    let StepKind::Action { with, .. } = &save.kind else {
        return Err(invalid("tool_producer_save_not_transport"));
    };
    let key = with
        .get("key")
        .ok_or_else(|| invalid("tool_producer_missing_publication_key"))?;
    let mut step = crate::cache_steps::cache_action_step(
        true,
        crate::cache_steps::TOOLS_RESTORE_USES,
        "tools",
        key,
        &[],
        &meta.descriptor.domain.payload(),
    )?;
    step.id = Some(
        velnor_actions_contract::StepId::new("velnor-tool-publication")
            .map_err(RenderError::Contract)?,
    );
    "Verify exact executable cache publication".clone_into(&mut step.name);
    step.condition = Some(format!(
        "always() && steps.{}.outputs.verified == 'true' && steps.{}.outputs.available == 'true' && steps.{}.outputs.changed == 'true'",
        meta.installation_step.as_str(),
        meta.after_step.as_str(),
        meta.after_step.as_str()
    ));
    if let StepKind::Action { with, .. } = &mut step.kind {
        with.insert("lookup-only".to_owned(), "true".to_owned());
    }
    Ok(step)
}

/// Admit exactly one closed producer sequence before normal consumer preparation.
pub(crate) fn validate_tool_producer(
    job: &Job,
    setup: &MiseSetup,
    records: &[CompiledSourceHelper],
) -> Result<bool, RenderError> {
    let Some(meta) = &job.tool_producer else {
        if job.steps.iter().any(|step| {
            matches!(&step.kind,
            StepKind::SourceBoundHelper { invocation, .. }
            if invocation.descriptor().operation() == SourceBoundOperation::ToolProducerReport)
        }) {
            return Err(invalid("tool_producer_missing_role"));
        }
        return Ok(false);
    };
    meta.validate().map_err(RenderError::Contract)?;
    if job.condition.as_ref() != Some(&meta.selection.condition(meta.descriptor.domain))
        || job.needs != meta.selection.needs(meta.descriptor.domain)
        || job.outputs != producer_outputs(meta)
    {
        return Err(invalid("tool_producer_scheduling_changed"));
    }
    if job.source_producer.is_some()
        || job.native_pages_deploy.is_some()
        || job.native_publish.is_some()
        || job.environment.is_some()
        || job.cache_mode != Some(velnor_actions_contract::CacheMode::Write)
        || job.runs_on != meta.descriptor.runs_on
        || velnor_actions_contract::tool_target_for_runner_label(&job.runs_on)
            != Some(meta.descriptor.target.as_str())
        || !job.permissions.as_ref().is_some_and(no_permissions)
    {
        return Err(invalid("tool_producer_private_authority"));
    }
    let installs: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.id.as_ref() == Some(&meta.installation_step))
        .collect();
    let [install] = installs.as_slice() else {
        return Err(invalid("tool_producer_install_binding"));
    };
    validate_install(install, meta, setup)?;
    crate::tool_producer_steps::validate_tool_descriptor(
        &meta.descriptor,
        setup,
        install,
        records,
    )?;
    let report = job
        .steps
        .last()
        .ok_or_else(|| invalid("tool_producer_missing_report"))?;
    validate_report(report, meta)?;
    let bootstrap = job
        .steps
        .get(3)
        .ok_or_else(|| invalid("tool_producer_missing_bootstrap"))?;
    validate_bootstrap(bootstrap, meta, setup)?;
    let domain = CacheSnapshotDomain::tool_domain(meta.descriptor.domain);
    let before = snapshots::registered_snapshot_step(domain, true, &meta.before_step, records)?;
    let after = snapshots::registered_snapshot_step(domain, false, &meta.after_step, records)?;
    for helper in [*install, bootstrap, report] {
        registered_helper(helper, records)?;
    }
    if job.steps
        != sequence(
            meta,
            (*install).clone(),
            bootstrap.clone(),
            before,
            after,
            report.clone(),
        )?
    {
        return Err(invalid("tool_producer_mixed_or_changed_computation"));
    }
    Ok(true)
}

fn registered_helper(step: &Step, records: &[CompiledSourceHelper]) -> Result<(), RenderError> {
    if !matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env }
        if records.iter().any(|record| record.invocation() == invocation && record.environment() == env))
    {
        return Err(invalid("tool_producer_helper_unregistered"));
    }
    Ok(())
}

fn no_permissions(permissions: &Permissions) -> bool {
    permissions
        == &Permissions {
            contents: PermissionLevel::None,
            actions: PermissionLevel::None,
            ..Permissions::default()
        }
}

/// Complete terminal report inventory, bound to the actual qualified report step.
#[must_use]
pub fn producer_outputs(
    meta: &PureToolProducer,
) -> Vec<velnor_actions_contract::workflow::JobOutput> {
    use velnor_actions_contract::workflow::{ActionOutput, JobOutput, StepOutputRef};
    [
        ActionOutput::CacheAvailable,
        ActionOutput::Verified,
        ActionOutput::ToolIdentity,
        ActionOutput::DescriptorIdentity,
        ActionOutput::Error,
    ]
    .into_iter()
    .map(|output| JobOutput {
        name: output.as_str().to_owned(),
        value: StepOutputRef {
            step_id: meta.report_step.clone(),
            output,
        },
    })
    .collect()
}

fn validate_install(
    step: &Step,
    meta: &PureToolProducer,
    setup: &MiseSetup,
) -> Result<(), RenderError> {
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        return Err(invalid("tool_producer_install_not_compiled"));
    };
    let operation = invocation.descriptor().operation();
    let permitted = match operation {
        SourceBoundOperation::MiseToolPrepare => true,
        SourceBoundOperation::RustPrepareRootLinux => {
            meta.descriptor.domain == ToolCacheDomain::Full
                && meta.descriptor.target == "x86_64-unknown-linux-gnu"
        }
        SourceBoundOperation::RustPrepareDesktopMac
        | SourceBoundOperation::RustPrepareDesktopSourceMac => {
            meta.descriptor.domain == ToolCacheDomain::Full
                && meta.descriptor.target == "aarch64-apple-darwin"
        }
        _ => false,
    };
    let isolated = [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("RUSTUP_AUTO_INSTALL", "0"),
    ]
    .into_iter()
    .all(|(key, value)| env.get(key).map(String::as_str) == Some(value));
    crate::commands::validate_env(env)?;
    crate::toolchain_env::reject_denied_step_keys(env)?;
    if !permitted
        || !isolated
        || operation == SourceBoundOperation::MiseToolPrepare
            && env.get("VELNOR_MISE_SHA256")
                != Some(
                    &setup
                        .bootstrap(meta.descriptor.domain, &meta.descriptor.runs_on)?
                        .binary_sha256,
                )
        || invocation.installed_selectors() != meta.descriptor.selectors
        || env.get("MISE_DATA_DIR").map(String::as_str) != Some(meta.descriptor.domain.root())
        || env.get("VELNOR_QUALIFIED_TOOL_IDENTITY")
            != Some(&meta.descriptor.qualification_identity)
        || step.id.as_ref() != Some(&meta.installation_step)
        || step.name != "Prepare and verify executable cache"
        || step.condition.is_some()
    {
        return Err(invalid("tool_producer_install_identity_changed"));
    }
    Ok(())
}

fn bootstrap_step(meta: &PureToolProducer, setup: &MiseSetup) -> Result<Step, RenderError> {
    if setup
        .bootstrap(meta.descriptor.domain, &meta.descriptor.runs_on)?
        .target
        != meta.descriptor.target
    {
        return Err(invalid("tool_producer_bootstrap_target_changed"));
    }
    crate::setup::mise_setup_step(setup, meta.descriptor.domain, &meta.descriptor.runs_on)
}

fn validate_bootstrap(
    step: &Step,
    meta: &PureToolProducer,
    setup: &MiseSetup,
) -> Result<(), RenderError> {
    if *step != bootstrap_step(meta, setup)? {
        return Err(invalid("tool_producer_bootstrap_binding_changed"));
    }
    Ok(())
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(reason.to_owned())
}

#[cfg(test)]
#[path = "cache_tool_roles_tests.rs"]
mod tests;
