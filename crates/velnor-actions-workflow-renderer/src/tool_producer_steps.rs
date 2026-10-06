//! Canonical closed-domain transports shared by pure producers and consumers.
use crate::{MiseSetup, RenderError, cache_p08};
use std::collections::BTreeSet;
use velnor_actions_contract::{
    Job, PureToolProducer, Step, StepKind, ToolCacheDescriptor, ToolCacheDomain,
};

#[path = "tool_consumer_environment.rs"]
mod environment;

#[path = "tool_producer_rust_source.rs"]
mod rust_source;
pub use rust_source::is_rust_source_preparation;

const IMAGE: &str = "-${{env.VELNOR_CACHE_IMAGE}}";

fn key(descriptor: &ToolCacheDescriptor) -> Result<String, RenderError> {
    descriptor.validate().map_err(RenderError::Contract)?;
    Ok(format!("{}{IMAGE}", descriptor.immutable_identity))
}

/// Platform compatibility is computed before either immutable transport.
/// # Errors
/// Rejects malformed descriptor inputs.
pub fn tool_producer_platform_step(descriptor: &ToolCacheDescriptor) -> Result<Step, RenderError> {
    descriptor.validate().map_err(RenderError::Contract)?;
    let mut step = cache_p08::payload::platform_step()?;
    bind_root(&mut step, descriptor.domain);
    Ok(step)
}

/// Read the newest immutable snapshot of this exact qualified payload.
/// # Errors
/// Rejects malformed transport identities.
pub fn tool_producer_restore_step(meta: &PureToolProducer) -> Result<Step, RenderError> {
    meta.validate().map_err(RenderError::Contract)?;
    restore_step(&meta.descriptor, &meta.restore_step)
}

fn restore_step(
    descriptor: &ToolCacheDescriptor,
    restore_id: &velnor_actions_contract::StepId,
) -> Result<Step, RenderError> {
    let key = key(descriptor)?;
    let lookup = format!("{key}-lookup-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}");
    let mut step = crate::cache_steps::cache_action_step(
        true,
        crate::cache_steps::TOOLS_RESTORE_USES,
        "tools",
        &lookup,
        &[format!("{key}-snapshot-")],
        &descriptor.domain.payload(),
    )?;
    step.name = format!("Restore {} tools", descriptor.domain.name());
    step.id = Some(restore_id.clone());
    Ok(step)
}

/// Only verified useful changes from the admitted pure job may be exported.
/// # Errors
/// Rejects malformed metadata or payload identities.
pub fn tool_producer_save_step(meta: &PureToolProducer) -> Result<Step, RenderError> {
    meta.validate().map_err(RenderError::Contract)?;
    let key = key(&meta.descriptor)?;
    let snapshot = format!(
        "{key}-snapshot-${{{{steps.{}.outputs.digest}}}}-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}",
        meta.after_step.as_str()
    );
    let mut step = crate::cache_steps::cache_action_step(
        false,
        crate::cache_steps::TOOLS_SAVE_USES,
        "tools",
        &snapshot,
        &[],
        &meta.descriptor.domain.payload(),
    )?;
    step.name = format!("Save {} tools", meta.descriptor.domain.name());
    step.id = Some(meta.save_step.clone());
    step.condition = Some(meta.save_condition());
    Ok(step)
}

fn bind_root(step: &mut Step, domain: ToolCacheDomain) {
    let (StepKind::Shell { env, .. } | StepKind::Action { env, .. }) = &mut step.kind else {
        return;
    };
    env.insert("MISE_DATA_DIR".to_owned(), domain.root().to_owned());
}

/// Describe the exact tool snapshots restored by the consumer job.
/// # Errors
/// Rejects malformed setup, target, and inferred installation selectors.
pub fn consumer_tool_descriptors(
    _job_id: &str,
    job: &Job,
    setup: &MiseSetup,
    target: &str,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<Vec<ToolCacheDescriptor>, RenderError> {
    cache_p08::validate_job_tool_records(job, records)?;
    if job.tool_producer.is_some() {
        return Ok(Vec::new());
    }
    if let Some(meta) = &job.source_producer {
        return Ok(meta.tool_cache.iter().cloned().collect());
    }
    if crate::early_plan::has_early_plan(job) {
        let early = job
            .steps
            .iter()
            .position(|step| {
                matches!(&step.kind,
            StepKind::Internal { operation } if operation == crate::steps::EARLY_PLAN_OPERATION)
            })
            .ok_or_else(|| {
                RenderError::InvalidWorkflow("tool_consumer_missing_early_plan".to_owned())
            })?;
        let mut planning = job.clone();
        planning.steps.truncate(early);
        let mut full = job.clone();
        drop(full.steps.drain(..=early));
        let mut descriptors = vec![descriptor(
            &planning,
            setup,
            target,
            ToolCacheDomain::Planning,
            records,
        )?];
        if !cache_p08::infer_job_selectors(&full).is_empty() {
            descriptors.push(descriptor(
                &full,
                setup,
                target,
                ToolCacheDomain::Full,
                records,
            )?);
        }
        return Ok(descriptors);
    }
    if cache_p08::infer_job_selectors(job).is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![descriptor(
        job,
        setup,
        target,
        ToolCacheDomain::Full,
        records,
    )?])
}

fn descriptor(
    job: &Job,
    setup: &MiseSetup,
    target: &str,
    domain: ToolCacheDomain,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<ToolCacheDescriptor, RenderError> {
    if velnor_actions_contract::tool_target_for_runner_label(&job.runs_on) != Some(target) {
        return Err(RenderError::InvalidWorkflow(
            "tool_consumer_actual_target_changed".to_owned(),
        ));
    }
    let mut identity = cache_p08::mise_cache_key_for_job(target, &setup.version, job, records)?;
    if domain != ToolCacheDomain::Full {
        identity = identity.replacen("mise-v3-", &format!("mise-v3-{}-", domain.name()), 1);
    }
    let qualifications: BTreeSet<_> = job
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation()
                    == velnor_actions_contract::SourceBoundOperation::MiseBootstrap =>
            {
                None
            }
            StepKind::Shell { env, .. } | StepKind::SourceBoundHelper { env, .. } => {
                env.get("VELNOR_QUALIFIED_TOOL_IDENTITY").cloned()
            }
            _ => None,
        })
        .collect();
    let qualifications: Vec<_> = qualifications.into_iter().collect();
    let [qualification_identity] = qualifications.as_slice() else {
        return Err(RenderError::InvalidWorkflow(
            "tool_consumer_qualification_missing_or_conflicting".to_owned(),
        ));
    };
    let descriptor = ToolCacheDescriptor {
        domain,
        target: target.to_owned(),
        runs_on: job.runs_on.clone(),
        qualification_identity: qualification_identity.clone(),
        selectors: cache_p08::infer_job_selectors(job),
        immutable_identity: identity,
    };
    descriptor.validate().map_err(RenderError::Contract)?;
    Ok(descriptor)
}

/// Canonical read-only bootstrap transport admitted inside native source jobs.
/// # Errors
/// Rejects malformed descriptors or noncanonical qualification pins.
pub fn tool_consumer_steps(
    descriptor: &ToolCacheDescriptor,
    setup: &MiseSetup,
) -> Result<Vec<Step>, RenderError> {
    let mut steps = tool_consumer_restore_steps(descriptor)?;
    steps.push(crate::setup::mise_setup_step(
        setup,
        descriptor.domain,
        &descriptor.runs_on,
    )?);
    Ok(steps)
}

/// Admit only an exact canonical read-only transport step from the bound domain.
/// # Errors
/// Rejects malformed descriptors or qualification pins.
pub fn is_tool_consumer_step(
    descriptor: &ToolCacheDescriptor,
    step: &Step,
    setup: &MiseSetup,
    target: &str,
) -> Result<bool, RenderError> {
    if descriptor.target != target {
        return Err(RenderError::InvalidWorkflow(
            "tool_consumer_target_changed".to_owned(),
        ));
    }
    Ok(tool_consumer_steps(descriptor, setup)?.contains(step))
}

/// Recompute publication identity from the exact compiled installation footprint.
/// # Errors
/// Rejects arbitrary namespaces, selectors, target, or qualification changes.
pub fn validate_tool_descriptor(
    descriptor_value: &ToolCacheDescriptor,
    setup: &MiseSetup,
    installation: &Step,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<(), RenderError> {
    let job = Job {
        cache_mode: None,
        display_name: "Tool descriptor admission".to_owned(),
        runs_on: descriptor_value.runs_on.clone(),
        timeout_minutes: velnor_actions_contract::JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps: vec![installation.clone()],
    };
    if descriptor(
        &job,
        setup,
        &descriptor_value.target,
        descriptor_value.domain,
        records,
    )? != *descriptor_value
    {
        return Err(RenderError::InvalidWorkflow(
            "tool_producer_descriptor_identity_changed".to_owned(),
        ));
    }
    Ok(())
}

/// Derive the immutable transport only from a compiled installation authority.
/// # Errors
/// Rejects unqualified footprint, host, selectors or environment bindings.
pub fn descriptor_for_record(
    record: &velnor_actions_contract::CompiledSourceHelper,
    runs_on: &str,
    target: &str,
    domain: ToolCacheDomain,
    setup: &MiseSetup,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<ToolCacheDescriptor, RenderError> {
    let step = crate::source_helper::source_helper_step(
        "Qualified tool identity",
        record,
        record.environment().clone(),
    )?;
    let job = Job {
        cache_mode: None,
        display_name: "Qualified tool identity".to_owned(),
        runs_on: runs_on.to_owned(),
        timeout_minutes: velnor_actions_contract::JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps: vec![step],
    };
    descriptor(&job, setup, target, domain, records)
}

/// Read-only transport for a compiled source producer with its own executable proof.
/// # Errors
/// Rejects malformed descriptor identities or restore bindings.
pub fn tool_consumer_restore_steps(
    descriptor: &ToolCacheDescriptor,
) -> Result<Vec<Step>, RenderError> {
    let restore_id = velnor_actions_contract::StepId::new(restore_id_for_domain(descriptor.domain))
        .map_err(RenderError::Contract)?;
    Ok(vec![
        tool_producer_platform_step(descriptor)?,
        restore_step(descriptor, &restore_id)?,
    ])
}

/// Admit a native bootstrap installer only within its exact compiled tool scope.
/// The generic helper registry still reconstructs its source and environment.
/// # Errors
/// Rejects altered root, footprint, host qualification or installation condition.
pub fn is_tool_consumer_installation(
    descriptor: &ToolCacheDescriptor,
    step: &Step,
    setup: &MiseSetup,
    records: &[velnor_actions_contract::CompiledSourceHelper],
) -> Result<bool, RenderError> {
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        return Ok(false);
    };
    if invocation.descriptor().operation()
        != velnor_actions_contract::SourceBoundOperation::MiseToolPrepare
    {
        return Ok(false);
    }
    if !environment::native_install_environment(env, descriptor.domain)
        || step.condition.is_some()
        || env.get("MISE_DATA_DIR").map(String::as_str) != Some(descriptor.domain.root())
        || env.get("VELNOR_MISE_SHA256")
            != Some(
                &setup
                    .bootstrap(descriptor.domain, &descriptor.runs_on)?
                    .binary_sha256,
            )
        || env.get("VELNOR_QUALIFIED_TOOL_IDENTITY") != Some(&descriptor.qualification_identity)
        || env.get("VELNOR_TOOL_CACHE_IDENTITY")
            != Some(&format!(
                "toolset@{}",
                velnor_actions_contract::digest_b3(descriptor.selectors.join("\0").as_bytes())
            ))
        || invocation.installed_selectors() != descriptor.selectors
    {
        return Err(RenderError::InvalidWorkflow(
            "tool_consumer_install_identity_changed".to_owned(),
        ));
    }
    validate_tool_descriptor(descriptor, setup, step, records)?;
    Ok(true)
}

fn restore_id_for_domain(domain: ToolCacheDomain) -> &'static str {
    match domain {
        ToolCacheDomain::Planning => "velnor-planning-tools-cache",
        ToolCacheDomain::Full => "velnor-tools-cache",
        ToolCacheDomain::NpmBootstrap => "velnor-npm-bootstrap-cache",
        ToolCacheDomain::BunBootstrap => "velnor-bun-bootstrap-cache",
        ToolCacheDomain::TofuBootstrap => "velnor-tofu-bootstrap-cache",
        ToolCacheDomain::GradleBootstrap => "velnor-gradle-bootstrap-cache",
    }
}
