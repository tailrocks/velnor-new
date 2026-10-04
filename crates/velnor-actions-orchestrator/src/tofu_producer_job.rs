use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CacheMode, CompiledSourceHelper, Job, JobTimeout, SourceProducer, SourceProducerRole, Step,
    StepId, ToolCacheDescriptor, ToolCacheDomain, ToolProducerSelection, VelnorConfig,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::MiseSetup;

use crate::{OrchestratorError, discover::Discovery, tofu_cache_source};
#[path = "tofu_producer_cohorts.rs"]
mod cohorts;
#[path = "tofu_producer_paths.rs"]
mod paths;
#[path = "tofu_producer_tool.rs"]
mod tool;
use paths::{candidate_path, tofu_cache_path};

const RESTORE_ID: &str = "velnor-tofu-provider-candidate";
const EXPORT_ID: &str = "velnor-tofu-provider-export";
const SAVE_ID: &str = "velnor-tofu-provider-save";

struct PendingProducer {
    descriptor: tofu_cache_source::ProviderExportDescriptor,
    label: String,
    root: String,
    mise: MiseSetup,
    selection: ToolProducerSelection,
}

pub(crate) fn source_key(
    descriptor: &tofu_cache_source::ProviderExportDescriptor,
    catalog: &ToolCatalog,
    label: &str,
    root: &str,
) -> Result<String, OrchestratorError> {
    let target = target_for_label(label)?;
    let candidate = candidate_path(root)?;
    let output = tofu_cache_path(root)?;
    let helper = crate::tofu_producer_source::compiled_helper(
        descriptor,
        target,
        catalog,
        &candidate,
        &output,
        env!("CARGO_PKG_VERSION"),
    )?;
    source_key_with_helper(descriptor, catalog, label, root, &helper)
}

fn source_key_with_helper(
    descriptor: &tofu_cache_source::ProviderExportDescriptor,
    catalog: &ToolCatalog,
    label: &str,
    root: &str,
    helper: &CompiledSourceHelper,
) -> Result<String, OrchestratorError> {
    let target = target_for_label(label)?;
    if descriptor.root != root {
        return Err(contract_error("tofu_provider_root_mismatch"));
    }
    velnor_actions_tofu::validate_normalized_root(root)?;
    let tofu_version = catalog.version(PinnedTool::Opentofu);
    velnor_actions_mise::validate_exact_version("opentofu", tofu_version)
        .map_err(contract_error)?;
    let descriptor =
        velnor_actions_contract::canonical_json_str(descriptor).map_err(contract_error)?;
    let invocation =
        velnor_actions_contract::canonical_json_str(helper.invocation()).map_err(contract_error)?;
    let identity = format!(
        "pure-producer-origin-v1\npublic-direct-provider-v1\ntarget={target}\n\
         tofu={tofu_version}\nroot={root}\nhelper-invocation={invocation}\n{descriptor}"
    );
    let digest = velnor_actions_contract::digest_b3(identity.as_bytes());
    let digest = digest.strip_prefix("b3-").unwrap_or(&digest);
    crate::tofu_cache::tofu_providers_cache_key(target, tofu_version, root, digest)
}

pub(crate) fn producer_id(key: &str) -> String {
    format!(
        "tofu-provider-source-{}",
        velnor_actions_contract::digest_b3(key.as_bytes())
    )
}

pub(crate) fn insert_producers(
    jobs: &mut BTreeMap<String, Job>,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    _label: &str,
    config: &VelnorConfig,
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let pending = cohorts::collect(jobs, discovery, catalog, config)?;
    let mut records = Vec::new();
    for (producer_id, pending) in pending {
        let (mut producer, producer_records) = producer_job(
            &pending.descriptor,
            catalog,
            &pending.label,
            &pending.root,
            &pending.mise,
            version,
            &pending.selection,
        )?;
        let metadata = producer
            .source_producer
            .as_ref()
            .ok_or_else(|| crate::internal::internal("tofu_source_role_missing"))?;
        if metadata.selection != pending.selection {
            return Err(crate::internal::internal("tofu_source_selection_mismatch"));
        }
        producer.condition = Some(metadata.condition());
        records.extend(producer_records);
        if jobs.insert(producer_id, producer).is_some() {
            return Err(crate::internal::internal("tofu_producer_job_collision"));
        }
    }
    Ok(records)
}

fn selection_for_tasks(tasks: &[&velnor_actions_contract::ProposedTask]) -> ToolProducerSelection {
    let tasks = tasks
        .iter()
        .map(|task| task.task_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    ToolProducerSelection {
        tasks,
        cargo_fallback: false,
        unconditional: false,
    }
}

fn tofu_root_for_tasks(
    tasks: &[&velnor_actions_contract::ProposedTask],
) -> Result<String, OrchestratorError> {
    let first = tasks
        .first()
        .ok_or_else(|| crate::internal::internal("tofu_empty_cohort"))?;
    let root = velnor_actions_tofu::normalized_root_for_proposal(first)?;
    for task in tasks {
        if velnor_actions_tofu::normalized_root_for_proposal(task)? != root {
            return Err(crate::internal::internal("tofu_mixed_root_cohort"));
        }
    }
    Ok(root.to_owned())
}

pub(crate) fn producer_job(
    descriptor: &tofu_cache_source::ProviderExportDescriptor,
    catalog: &ToolCatalog,
    label: &str,
    root: &str,
    mise: &MiseSetup,
    generator_version: &str,
    selection: &ToolProducerSelection,
) -> Result<(Job, Vec<CompiledSourceHelper>), OrchestratorError> {
    let target = target_for_label(label)?;
    let candidate = candidate_path(root)?;
    let output = tofu_cache_path(root)?;
    let base_helper = crate::tofu_producer_source::compiled_helper(
        descriptor,
        target,
        catalog,
        &candidate,
        &output,
        generator_version,
    )?;
    let key = source_key_with_helper(descriptor, catalog, label, root, &base_helper)?;
    let mut helper_env = producer_env_with_paths(root, target, &candidate, &output)?;
    helper_env.insert("VELNOR_SOURCE_IDENTITY".to_owned(), key.clone());
    let helper = base_helper.with_environment(helper_env);
    let (tool, installation) = tool::preparation(catalog, label, target, mise, generator_version)?;
    let ownership = crate::tofu_ownership_helper::record(root, generator_version)?;
    let metadata = source_metadata(&key, &tool, selection)?;
    let mut steps = owned_tool_steps(&ownership, &installation, &tool, mise)?;
    let restore = restore_step(&key, &candidate)?;
    let export = export_step(&helper)?;
    let save = save_step(&metadata, &output)?;
    let publication =
        crate::workloads::cache::source_report::publication_step(&metadata, &[output])?;
    let report = crate::workloads::cache::source_report::step(&metadata, generator_version)?;
    let report_record =
        crate::workloads::cache::source_report::record(&metadata, generator_version)?;
    steps.extend([restore, export, save, publication, report]);
    let job = Job {
        display_name: "Public OpenTofu providers".to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: selection.needs(ToolCacheDomain::TofuBootstrap),
        condition: Some(metadata.condition()),
        cache_mode: Some(CacheMode::Write),
        permissions: Some(tool::empty_permissions()),
        environment: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: Some(metadata.clone()),
        native_pages_deploy: None,
        native_publish: None,
        outputs: crate::workloads::cache::source_report::outputs(&metadata),
        steps,
    };
    let bootstrap = mise
        .bootstrap(ToolCacheDomain::TofuBootstrap, label)?
        .helper
        .clone();
    Ok((
        job,
        vec![bootstrap, installation, ownership, helper, report_record],
    ))
}

fn owned_tool_steps(
    ownership: &CompiledSourceHelper,
    installation: &CompiledSourceHelper,
    tool: &ToolCacheDescriptor,
    mise: &MiseSetup,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps =
        velnor_actions_workflow_renderer::tool_producer_steps::tool_consumer_steps(tool, mise)?;
    steps.extend([
        tool::preparation_step(installation)?,
        crate::tofu_ownership_helper::step(ownership)?,
    ]);
    Ok(steps)
}

fn source_metadata(
    key: &str,
    tool: &ToolCacheDescriptor,
    selection: &ToolProducerSelection,
) -> Result<SourceProducer, OrchestratorError> {
    selection.validate().map_err(contract_error)?;
    if selection.tasks.is_empty() || selection.cargo_fallback || selection.unconditional {
        return Err(contract_error("tofu_source_selection_not_explicit"));
    }
    let metadata = SourceProducer {
        role: SourceProducerRole::Tofu,
        selection: selection.clone(),
        tool_cache: Some(tool.clone()),
        source_identity: key.to_owned(),
        verification_step: StepId::new(EXPORT_ID).map_err(contract_error)?,
        restore_step: StepId::new(RESTORE_ID).map_err(contract_error)?,
        save_step: StepId::new(SAVE_ID).map_err(contract_error)?,
        publication_step: StepId::new("velnor-tofu-provider-publication")
            .map_err(contract_error)?,
        report_step: StepId::new(crate::workloads::cache::source_report::REPORT_ID)
            .map_err(contract_error)?,
    };
    metadata.validate()?;
    Ok(metadata)
}

fn restore_step(key: &str, candidate: &str) -> Result<Step, OrchestratorError> {
    use velnor_actions_actionlint::PinnedActionRef;
    use velnor_actions_actionlint::actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION};

    let uses = PinnedActionRef::new(
        "actions/cache",
        Some("restore"),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )
    .map_err(OrchestratorError::from)?
    .uses_value();
    let mut step = velnor_actions_workflow_renderer::action_step(
        "Restore Tofu provider candidate",
        &uses,
        BTreeMap::from([
            ("key".to_owned(), key.to_owned()),
            ("restore-keys".to_owned(), String::new()),
            ("path".to_owned(), candidate.to_owned()),
        ]),
    )?;
    step.id = Some(StepId::new(RESTORE_ID).map_err(contract_error)?);
    Ok(step)
}

fn export_step(helper: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Export verified OpenTofu providers",
        helper,
        helper.environment().clone(),
    )?;
    step.id = Some(StepId::new(EXPORT_ID).map_err(contract_error)?);
    Ok(step)
}

fn save_step(meta: &SourceProducer, output: &str) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::tofu_cache::tofu_providers_save_step(
        &meta.source_identity,
        output,
    )?;
    step.id = Some(StepId::new(SAVE_ID).map_err(contract_error)?);
    step.condition = Some(meta.save_condition());
    Ok(step)
}

fn producer_env(root: &str, target: &str) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let home = crate::tofu_ownership_helper::producer_home(root)?;
    let mut env = BTreeMap::from([
        ("HOME".to_owned(), home.clone()),
        ("XDG_CONFIG_HOME".to_owned(), format!("{home}/config")),
        ("XDG_DATA_HOME".to_owned(), format!("{home}/data")),
        ("XDG_CACHE_HOME".to_owned(), format!("{home}/cache")),
        (
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::TofuBootstrap.root().to_owned(),
        ),
        ("VELNOR_TOFU_PROVIDER_TARGET".to_owned(), target.to_owned()),
        ("TF_IN_AUTOMATION".to_owned(), "1".to_owned()),
        ("TF_INPUT".to_owned(), "0".to_owned()),
    ]);
    env.extend(
        velnor_actions_mise::ISOLATION_ENV
            .iter()
            .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV.iter())
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    Ok(env)
}

fn producer_env_with_paths(
    root: &str,
    target: &str,
    candidate: &str,
    output: &str,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut env = producer_env(root, target)?;
    env.extend(crate::tofu_producer_source::producer_environment(
        Some(candidate),
        output,
    )?);
    Ok(env)
}

fn target_for_label(label: &str) -> Result<&'static str, OrchestratorError> {
    let target = velnor_actions_contract::target_for_runner_label(label)
        .ok_or_else(|| contract_error(format!("unsupported_target_for_runner:{label}")))?;
    velnor_actions_contract::is_supported_target(target)
        .then_some(target)
        .ok_or_else(|| contract_error(format!("unsupported_target:{target}")))
}

fn contract_error(error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}
