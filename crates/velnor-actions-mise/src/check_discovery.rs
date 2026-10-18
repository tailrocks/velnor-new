use super::{invalid, repository_path};
use crate::MiseError;
#[path = "check_tool_projection.rs"]
mod projection;
#[path = "check_qualified_tools.rs"]
mod qualified;
#[path = "check_task_validation.rs"]
mod task_validation;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, MiseCheck, MiseTaskSource, QualifiedTool,
    RustBinaryPreviewProducer, VerificationRunner,
};
use velnor_actions_contract::graph::{CachePolicy, ResourceClass, ResourceDemand};
use velnor_actions_contract::propose::{IdentityInputs, ProposedTask};

/// A static check obligation and its source-bound task projection.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredCheck {
    /// Validated root configuration row.
    pub check: MiseCheck,
    /// Neutral planned obligation; opaque checks never reuse results.
    pub proposal: ProposedTask,
    /// Repository inputs including the complete native task source.
    pub config_inputs: Vec<String>,
    /// Original task table bytes, with native dependency relationships.
    pub task_config: String,
    /// Exact selectors from the explicitly qualified installation closure.
    pub tool_specs: Vec<String>,
    /// Explicit qualified installation closure in prerequisite order.
    pub qualified_tools: Vec<QualifiedTool>,
    /// Canonical full declaration, option, dependency and source fingerprint.
    pub qualification_digest: String,
    /// Full original config bytes: rechecked before runtime projection.
    pub config_source: String,
}

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
        .map(|check| discover(root, check, qualified_tools, None, None))
        .collect()
}

/// Discover checks while consuming the caller's shared execution deadline.
/// # Errors
/// Rejects expired deadlines and the same invalid task/source states as `discover_checks`.
pub fn discover_checks_until(
    root: &Path,
    checks: &[MiseCheck],
    qualified_tools: &[QualifiedTool],
    deadline: crate::CheckDeadline,
) -> Result<Vec<DiscoveredCheck>, MiseError> {
    checks
        .iter()
        .map(|check| discover(root, check, qualified_tools, Some(deadline), None))
        .collect()
}

/// Discover a preview producer from its explicitly configured native task source.
///
/// The producer shares named-check source parsing, task projection, and qualified
/// tool closure resolution. It is a task binding only; it emits no CI check or
/// evidence report.
/// # Errors
/// Rejects unsafe source paths, unsupported task syntax, missing tasks, and
/// incomplete qualified-tool closure.
pub fn discover_preview_producer(
    root: &Path,
    producer: &RustBinaryPreviewProducer,
    qualified_tools: &[QualifiedTool],
    deadline: crate::CheckDeadline,
) -> Result<DiscoveredCheck, MiseError> {
    validate_explicit_source(&producer.source)?;
    let (label, platform) = match producer.runner {
        VerificationRunner::LinuxX64 => ("ubuntu-26.04", CheckPlatform::LinuxX64),
        VerificationRunner::MacosArm64 => ("macos-15", CheckPlatform::MacosArm64),
        VerificationRunner::Macos26Arm64 => ("macos-26", CheckPlatform::MacosArm64),
    };
    let check = MiseCheck {
        id: "preview-producer".to_owned(),
        task: producer.task.clone(),
        directory: producer.source.working_directory.clone(),
        runner: CheckRunner {
            label: label.to_owned(),
            platform,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: producer.inputs.clone(),
        tools: producer.tools.clone(),
        system_tools: producer.system_tools.clone(),
        evidence: None,
        timeout_minutes: u32::from(producer.timeout_minutes),
    };
    discover(
        root,
        &check,
        qualified_tools,
        Some(deadline),
        Some(&producer.source),
    )
}

fn discover(
    root: &Path,
    check: &MiseCheck,
    qualified_tools: &[QualifiedTool],
    deadline: Option<crate::CheckDeadline>,
    source_override: Option<&MiseTaskSource>,
) -> Result<DiscoveredCheck, MiseError> {
    if !velnor_actions_contract::config::is_valid_mise_task_name(&check.task) {
        return Err(invalid("task", "invalid_named_task"));
    }
    let (source, bytes) = read_source(root, check, source_override, deadline)?;
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
    source_override: Option<&MiseTaskSource>,
    deadline: Option<crate::CheckDeadline>,
) -> Result<(String, String), MiseError> {
    if let Some(source) = source_override {
        if check.directory != source.working_directory {
            return Err(invalid("mise_source", "working_directory_mismatch"));
        }
        validate_explicit_source(source)?;
        repository_path(root, &source.working_directory)?;
        let path = repository_path(root, &source.mise_config)?;
        let bytes = crate::checks::file_read::read_text(
            &path,
            velnor_actions_contract::MAX_CHECK_SOURCE_BYTES,
            deadline,
            "mise_config",
        )?;
        return Ok((source.mise_config.clone(), bytes));
    }
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
    let bytes = crate::checks::file_read::read_text(
        &path,
        velnor_actions_contract::MAX_CHECK_SOURCE_BYTES,
        deadline,
        "mise_config",
    )?;
    Ok((source, bytes))
}

fn validate_explicit_source(source: &MiseTaskSource) -> Result<(), MiseError> {
    let config = source.mise_config.as_str();
    let working = source.working_directory.as_str();
    let valid_path = |path: &str, root_allowed: bool| {
        (root_allowed && path == ".")
            || (!path.is_empty()
                && path.len() <= 1024
                && !path.starts_with('/')
                && !path.ends_with('/')
                && path.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-')
                })
                && path
                    .split('/')
                    .all(|segment| !segment.is_empty() && segment != "." && segment != ".."))
    };
    if !valid_path(config, false)
        || !(config == crate::toolfiles::MISE_TOML_FILE
            || config
                .strip_suffix(crate::toolfiles::MISE_TOML_FILE)
                .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('/')))
    {
        return Err(invalid("mise_source", "bad_config_path"));
    }
    if !valid_path(working, true) {
        return Err(invalid("mise_source", "bad_working_directory"));
    }
    let config_suffix = format!("/{}", crate::toolfiles::MISE_TOML_FILE);
    let config_directory = config
        .strip_suffix(&config_suffix)
        .filter(|directory| !directory.is_empty())
        .unwrap_or(".");
    if config_directory != "."
        && working != config_directory
        && !working
            .strip_prefix(config_directory)
            .is_some_and(|suffix| suffix.starts_with('/'))
    {
        return Err(invalid(
            "mise_source",
            "working_directory_outside_config_tree",
        ));
    }
    Ok(())
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

impl DiscoveredCheck {
    /// Shared canonical fingerprint over full declarations and exact selected specs.
    /// # Errors
    /// Serialization failures reject the qualification identity.
    pub fn qualification_fingerprint(
        tools: &[QualifiedTool],
        specs: &[String],
    ) -> Result<String, MiseError> {
        qualified::fingerprint(tools, specs)
    }

    /// Refuse altered declarations, selectors or proposal qualification bindings.
    /// # Errors
    /// Every discovered qualification field must retain its exact canonical identity.
    pub fn verify_qualification_identity(&self) -> Result<(), MiseError> {
        let digest = Self::qualification_fingerprint(&self.qualified_tools, &self.tool_specs)?;
        let expected_flag = format!("qualified_tools:{digest}");
        let mut specs: Vec<&str> = self
            .proposal
            .identity
            .flags
            .iter()
            .filter_map(|flag| flag.strip_prefix("tool:"))
            .collect();
        specs.sort_unstable();
        let actual: Vec<&str> = self.tool_specs.iter().map(String::as_str).collect();
        if digest != self.qualification_digest
            || !self.proposal.identity.flags.contains(&expected_flag)
            || specs != actual
        {
            return Err(invalid("qualified_tool_identity", "qualification_changed"));
        }
        Ok(())
    }
    /// Source-free typed singleton config for one resolved installation record.
    /// # Errors
    /// Unknown IDs or invalid declaration slots fail before config construction.
    pub fn qualified_tool_config(&self, id: &str) -> Result<String, MiseError> {
        let tool = self
            .qualified_tools
            .iter()
            .find(|tool| tool.id == id)
            .ok_or_else(|| invalid("qualified_tool_projection", "unselected_tool_id"))?;
        projection::config_for(tool)
    }
    /// Exact selected Rust channel, including explicit repository qualification.
    #[must_use]
    pub fn selected_rust_version(&self) -> Option<&str> {
        self.tool_specs
            .iter()
            .find_map(|spec| spec.strip_prefix("rust@"))
    }
}
