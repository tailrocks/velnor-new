//! Static named-check discovery over task-config sources.
//!
//! Reads the single owned task config, projects native tasks, and binds
//! the qualified installation closure. Only Mise executes anything.

use velnor_actions_mise_core::MiseError;
use velnor_actions_mise_core::checks::{DiscoveredCheck, invalid, repository_path};
mod qualified;
mod task_validation;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use velnor_actions_contract::config::{MiseCheck, QualifiedTool};
use velnor_actions_contract::graph::{CachePolicy, ResourceClass, ResourceDemand};
use velnor_actions_contract::propose::{IdentityInputs, ProposedTask};

/// Inspect named task definitions without invoking Mise or consumer code.
/// Unsupported includes/file tasks/templated discovery fail explicitly.
/// # Errors
/// Rejects missing tasks, unsupported task semantics, and escaped inputs.
pub fn discover_checks(
    root: &Path,
    checks: &[MiseCheck],
    qualified_tools: &[QualifiedTool],
) -> Result<Vec<DiscoveredCheck>, MiseError> {
    checks
        .iter()
        .map(|check| discover(root, check, qualified_tools, None))
        .collect()
}

/// Discover checks while consuming the caller's shared execution deadline.
/// # Errors
/// Rejects expired deadlines and the same invalid task/source states as `discover_checks`.
pub fn discover_checks_until(
    root: &Path,
    checks: &[MiseCheck],
    qualified_tools: &[QualifiedTool],
    deadline: velnor_actions_mise_core::CheckDeadline,
) -> Result<Vec<DiscoveredCheck>, MiseError> {
    checks
        .iter()
        .map(|check| discover(root, check, qualified_tools, Some(deadline)))
        .collect()
}

fn discover(
    root: &Path,
    check: &MiseCheck,
    qualified_tools: &[QualifiedTool],
    deadline: Option<velnor_actions_mise_core::CheckDeadline>,
) -> Result<DiscoveredCheck, MiseError> {
    if !velnor_actions_contract::config::is_valid_mise_task_name(&check.task) {
        return Err(invalid("task", "invalid_named_task"));
    }
    let (source, bytes) = read_source(root, check, deadline)?;
    let doc = task_validation::validated_tasks(&bytes, &check.task)?;
    let projection = task_validation::task_projection(&doc, &bytes);
    let mut inputs = check.inputs.clone();
    inputs.push(source.clone());
    inputs.sort();
    inputs.dedup();
    for input in &inputs {
        repository_path(root, input)?;
    }
    let resolved = qualified::resolve(qualified_tools, &check.tools, check)?;
    let proposal = propose_check(
        check,
        &source,
        &inputs,
        &resolved.specs,
        &resolved.fingerprint,
    )?;
    Ok(DiscoveredCheck {
        check: check.clone(),
        proposal,
        config_inputs: inputs,
        task_config: projection,
        tool_specs: resolved.specs,
        qualified_tools: resolved.declarations,
        qualification_digest: resolved.fingerprint,
        config_source: bytes,
    })
}

fn read_source(
    root: &Path,
    check: &MiseCheck,
    deadline: Option<velnor_actions_mise_core::CheckDeadline>,
) -> Result<(String, String), MiseError> {
    repository_path(root, &check.directory)?;
    let prefix = if check.directory == "." {
        String::new()
    } else {
        format!("{}/", check.directory)
    };
    let files: Vec<String> = [
        crate::toolfiles::MISE_TOML_FILE,
        crate::toolfiles::DOT_MISE_TOML_FILE,
    ]
    .iter()
    .map(|f| format!("{prefix}{f}"))
    .filter(|f| root.join(f).exists())
    .collect();
    if files.len() != 1 {
        return Err(invalid("mise_config", "exactly_one_task_config_required"));
    }
    let source = files[0].clone();
    let path = repository_path(root, &source)?;
    let bytes = velnor_actions_mise_core::checks::read_text(
        &path,
        velnor_actions_contract::MAX_CHECK_SOURCE_BYTES,
        deadline,
        "mise_config",
    )?;
    Ok((source, bytes))
}

fn check_identity(
    check: &MiseCheck,
    source: &str,
    inputs: &[String],
    specs: &[String],
    qualification_digest: &str,
) -> Result<IdentityInputs, MiseError> {
    let mut flags: Vec<String> = specs.iter().map(|s| format!("tool:{s}")).collect();
    flags.push(format!("qualified_tools:{qualification_digest}"));
    for pin in &check.system_tools {
        flags.push(format!(
            "system_tool:{:?}:{}:{}",
            pin.kind, pin.version, pin.build
        ));
    }
    flags.push(format!("runner:{}", check.runner.label));
    flags.push(format!("executor:{:?}", check.runner.executor));
    let runner_bytes = velnor_actions_contract::canonical_json_bytes(&check.runner)
        .map_err(|e| invalid("check_runner_identity", e.to_string()))?;
    flags.push(format!(
        "runner_profile:{}",
        velnor_actions_contract::digest_b3(&runner_bytes)
    ));
    flags.sort();
    Ok(IdentityInputs {
        unit_id: check.id.clone(),
        unit_key: check.id.clone(),
        unit_path: source.to_owned(),
        project_root: check.directory.clone(),
        target: check.runner.platform.target().into(),
        features: vec![],
        flags,
        compile_driver: "mise".into(),
        test_runner: "mise".into(),
        environment: BTreeMap::new(),
        declared_inputs: inputs.to_vec(),
        undeclared_reads: true,
    })
}

fn propose_check(
    check: &MiseCheck,
    source: &str,
    inputs: &[String],
    specs: &[String],
    qualification_digest: &str,
) -> Result<ProposedTask, MiseError> {
    let proposal = ProposedTask {
        task_id: format!("stack/mise/{}/check/default", check.id),
        stack_id: "mise".into(),
        component_id: check.id.clone(),
        task_kind: "check".into(),
        configuration: "default".into(),
        depends_on: vec![],
        gated_by: vec![],
        reads: inputs.to_vec(),
        writes: vec![check.directory.clone()],
        outputs: vec![],
        resource: ResourceDemand {
            class: ResourceClass::Exclusive,
            cpu_milli: None,
            memory_mb: None,
            needs_network: true,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: check_identity(check, source, inputs, specs, qualification_digest)?,
        payload: vec![OsString::from(&check.task)],
        display_name: check.id.clone(),
        uses_clock: true,
        uses_random: true,
        no_targets: false,
        runner_profile: check.runner.label.clone(),
    };
    proposal
        .validate()
        .map_err(|e| invalid("check_proposal", e.to_string()))?;
    Ok(proposal)
}
