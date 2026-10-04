//! Exact local installation authority precedes immutable consumer descriptors.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, Job, SourceBoundOperation, Step, StepKind, ToolCacheDomain,
};
use velnor_actions_mise::{
    MISE_GLOBAL_FLAGS, ToolCatalog,
    catalog::{qualification::DistributionHost, rust_prepare, tool_prepare},
};

use crate::OrchestratorError;

/// Every ordinary consumer retains verified local preparation on a cold cache.
pub(super) fn normalize(
    jobs: &mut BTreeMap<String, Job>,
    catalog: &ToolCatalog,
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let mut records = Vec::new();
    for job in jobs.values_mut() {
        if job.source_producer.is_some() || job.tool_producer.is_some() {
            continue;
        }
        if let Some(early) = job.steps.iter().position(|step| {
            matches!(&step.kind, StepKind::Internal { operation }
                if operation == velnor_actions_workflow_renderer::steps::EARLY_PLAN_OPERATION)
        }) {
            let mut suffix = job.clone();
            suffix.steps = job.steps.split_off(early + 1);
            let boundary = job
                .steps
                .pop()
                .ok_or_else(|| invalid("consumer_early_boundary_missing"))?;
            prepare_phase(
                job,
                catalog,
                ToolCacheDomain::Planning,
                version,
                &mut records,
            )?;
            prepare_phase(
                &mut suffix,
                catalog,
                ToolCacheDomain::Full,
                version,
                &mut records,
            )?;
            job.steps.push(boundary);
            job.steps.extend(suffix.steps);
        } else {
            prepare_phase(job, catalog, ToolCacheDomain::Full, version, &mut records)?;
        }
    }
    Ok(records)
}

fn prepare_phase(
    job: &mut Job,
    catalog: &ToolCatalog,
    domain: ToolCacheDomain,
    version: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<(), OrchestratorError> {
    let selectors = velnor_actions_workflow_renderer::cache_p08::infer_job_selectors(job);
    if selectors.is_empty() {
        return Ok(());
    }
    let host = host_for_job(job)?;
    let scoped = catalog_for_selectors(catalog, &selectors, host)?;
    let installers: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter_map(|(at, step)| is_installer(step).then_some(at))
        .collect();
    if installers.is_empty() {
        let record = tool_prepare::helper_for_tools(&scoped, domain, host, &selectors, version)
            .map_err(contract)?;
        let mut step = helper_step("Prepare pinned tools", &record)?;
        let first_use = first_use(job).ok_or_else(|| invalid("consumer_tools_without_use"))?;
        step.condition = preparation_condition(job);
        job.steps.insert(first_use, step);
        remember(records, record);
    } else {
        for at in installers {
            let record =
                normalized_record(&job.steps[at], &scoped, domain, host, &selectors, version)?;
            job.steps[at].kind = helper_step(&job.steps[at].name, &record)?.kind;
            remember(records, record);
        }
        ensure_before_use(job)?;
    }
    for step in &mut job.steps {
        if let StepKind::Shell { run, env } = &mut step.kind
            && mise_operation(run).is_some()
        {
            env.insert("MISE_DATA_DIR".to_owned(), domain.root().to_owned());
        }
    }
    Ok(())
}

fn normalized_record(
    step: &Step,
    catalog: &ToolCatalog,
    domain: ToolCacheDomain,
    host: DistributionHost,
    selectors: &[String],
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let expected = tool_prepare::helper_for_tools(catalog, domain, host, selectors, version)
        .map_err(contract)?;
    if let StepKind::SourceBoundHelper { invocation, env } = &step.kind {
        let original =
            tool_prepare::record_for_invocation(invocation, env, version).map_err(contract)?;
        let mut installed = invocation.installed_selectors().to_vec();
        installed.sort();
        if installed == selectors
            && env
                .get("MISE_DATA_DIR")
                .is_some_and(|root| root == domain.root())
            && (invocation.descriptor().operation() != SourceBoundOperation::MiseToolPrepare
                || original == expected)
        {
            return Ok(original);
        }
        if invocation
            .args()
            .first()
            .is_some_and(|argument| argument == "planning-bootstrap")
        {
            let install: Vec<_> = std::iter::once("mise")
                .chain(MISE_GLOBAL_FLAGS.iter().copied())
                .chain(std::iter::once("install"))
                .map(str::to_owned)
                .chain(selectors.iter().cloned())
                .collect();
            if domain != ToolCacheDomain::Full {
                return Err(invalid("consumer_planning_rust_forbidden"));
            }
            return rust_prepare::helper_for_install(
                catalog,
                rust_prepare::RustPrepareDomain::PlanningBootstrap,
                &install,
                version,
            )
            .map_err(contract);
        }
    }
    Ok(expected)
}

fn ensure_before_use(job: &mut Job) -> Result<(), OrchestratorError> {
    let installer = job
        .steps
        .iter()
        .position(is_installer)
        .ok_or_else(|| invalid("consumer_install_missing"))?;
    if let Some(first) = first_use(job)
        && installer > first
    {
        let step = job.steps.remove(installer);
        job.steps.insert(first, step);
    }
    Ok(())
}

fn first_use(job: &Job) -> Option<usize> {
    job.steps.iter().position(uses_tools)
}

fn uses_tools(step: &Step) -> bool {
    !is_installer(step)
        && match &step.kind {
            StepKind::Shell { run, .. } => mise_operation(run) == Some("exec"),
            StepKind::SourceBoundHelper { invocation, .. } => {
                !invocation.installed_selectors().is_empty()
            }
            _ => false,
        }
}

fn preparation_condition(job: &Job) -> Option<String> {
    let mut conditions = Vec::new();
    for step in job.steps.iter().filter(|step| uses_tools(step)) {
        let condition = step.condition.as_ref()?;
        if !conditions.contains(condition) {
            conditions.push(condition.clone());
        }
    }
    conditions
        .into_iter()
        .reduce(|left, right| format!("({left}) || ({right})"))
}

fn is_installer(step: &Step) -> bool {
    match &step.kind {
        StepKind::Shell { run, .. } => mise_operation(run) == Some("install"),
        StepKind::SourceBoundHelper { invocation, .. } => matches!(
            invocation.descriptor().operation(),
            SourceBoundOperation::MiseToolPrepare
                | SourceBoundOperation::RustPrepareRootLinux
                | SourceBoundOperation::RustPrepareDesktopMac
                | SourceBoundOperation::RustPrepareDesktopSourceMac
        ),
        _ => false,
    }
}

/// Only the Mise owner's literal argv prefix qualifies; shell text stays opaque.
fn mise_operation(run: &[String]) -> Option<&str> {
    let scrub = velnor_actions_workflow_renderer::toolchain_env::with_env_unset_argv(&[]);
    let run = run.strip_prefix(scrub.as_slice()).unwrap_or(run);
    if run.first().map(String::as_str) != Some("mise") {
        return None;
    }
    let tail = run.get(1..)?;
    if !tail
        .iter()
        .map(String::as_str)
        .take(MISE_GLOBAL_FLAGS.len())
        .eq(MISE_GLOBAL_FLAGS.iter().copied())
    {
        return None;
    }
    match tail.get(MISE_GLOBAL_FLAGS.len()).map(String::as_str) {
        Some(operation @ ("install" | "exec")) => Some(operation),
        _ => None,
    }
}

fn host_for_job(job: &Job) -> Result<DistributionHost, OrchestratorError> {
    match velnor_actions_contract::tool_target_for_runner_label(&job.runs_on) {
        Some("x86_64-unknown-linux-gnu") => Ok(DistributionHost::LinuxAmd64),
        Some("aarch64-unknown-linux-gnu") => Ok(DistributionHost::LinuxArm64),
        Some("aarch64-apple-darwin") => Ok(DistributionHost::MacosArm64),
        _ => Err(invalid("consumer_tool_host_unknown")),
    }
}

fn catalog_for_selectors(
    catalog: &ToolCatalog,
    selectors: &[String],
    host: DistributionHost,
) -> Result<ToolCatalog, OrchestratorError> {
    if host.abi() == catalog.rust_host().target_triple()
        && selectors.contains(&catalog.native_tool_spec(host, catalog.compiler_tool())?)
    {
        return Ok(catalog.clone());
    }
    for scoped in [
        catalog.for_native_kind("native_xcode_project_ci"),
        catalog.for_native_source_kind("native_xcode_project_ci"),
    ] {
        let scoped = scoped.map_err(contract)?;
        if host.abi() == scoped.rust_host().target_triple()
            && selectors.contains(&scoped.native_tool_spec(host, scoped.compiler_tool())?)
        {
            return Ok(scoped);
        }
    }
    Ok(ToolCatalog::pinned())
}

fn helper_step(name: &str, record: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::source_helper::source_helper_step(
        name,
        record,
        record.environment().clone(),
    )
    .map_err(OrchestratorError::from)
}

fn remember(records: &mut Vec<CompiledSourceHelper>, record: CompiledSourceHelper) {
    if !records.contains(&record) {
        records.push(record);
    }
}

fn contract(error: velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

fn invalid(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}
