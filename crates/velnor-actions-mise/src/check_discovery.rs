use super::{invalid, repository_path};
use crate::MiseError;
use crate::toml_scan::{TomlValue, parse_toml};
#[path = "check_tool_projection.rs"]
mod projection;
#[path = "check_qualified_tools.rs"]
mod qualified;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;
use velnor_actions_contract::config::{MiseCheck, QualifiedTool};
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
        .map(|check| discover(root, check, qualified_tools))
        .collect()
}

fn discover(
    root: &Path,
    check: &MiseCheck,
    qualified_tools: &[QualifiedTool],
) -> Result<DiscoveredCheck, MiseError> {
    if !velnor_actions_contract::config::is_valid_mise_task_name(&check.task) {
        return Err(invalid("task", "invalid_named_task"));
    }
    let (source, bytes) = read_source(root, check)?;
    let doc = validated_tasks(&bytes, &check.task)?;
    let projection = task_projection(&doc, &bytes);
    let mut inputs = check.inputs.clone();
    inputs.push(source.clone());
    inputs.sort();
    inputs.dedup();
    for input in &inputs {
        repository_path(root, input)?;
    }
    let resolved = qualified::resolve(qualified_tools, &check.tools, check.runner.platform)?;
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

fn read_source(root: &Path, check: &MiseCheck) -> Result<(String, String), MiseError> {
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
    let bytes = std::fs::read_to_string(repository_path(root, &source)?)
        .map_err(|e| invalid("mise_config", e.to_string()))?;
    Ok((source, bytes))
}

const TASK_FIELDS: &[&str] = &[
    "run",
    "description",
    "usage",
    "alias",
    "depends",
    "depends_post",
    "wait_for",
    "sources",
    "outputs",
    "hide",
    "quiet",
    "silent",
    "raw",
    "shell",
];

fn validated_tasks(bytes: &str, selected: &str) -> Result<crate::toml_scan::TomlDoc, MiseError> {
    let doc = parse_toml(bytes)
        .map_err(|e| invalid("mise_config", format!("{}:{}", e.line, e.problem)))?;
    let mut names = BTreeSet::new();
    let mut edges = BTreeMap::<String, Vec<String>>::new();
    for (path, _) in &doc.sections {
        if path.first().is_some_and(|p| p == "tasks") {
            if path.len() != 2 {
                return Err(invalid("mise_tasks", "nested_task_tables_unsupported"));
            }
            if !velnor_actions_contract::config::is_valid_mise_task_name(&path[1]) {
                return Err(invalid("mise_task_name", &path[1]));
            }
            names.insert(path[1].clone());
        }
    }
    let mut aliases = BTreeSet::new();
    for assignment in &doc.assignments {
        if let Some(refs) = validate_assignment(assignment)? {
            let task = &assignment.path[1];
            if assignment.path[2] == "alias" {
                for alias in refs {
                    if names.contains(&alias) || !aliases.insert(alias.clone()) {
                        return Err(invalid("mise_task_alias", "duplicate_or_shadowed_alias"));
                    }
                }
            } else {
                edges.entry(task.clone()).or_default().extend(refs);
            }
        }
    }
    names.extend(aliases);
    for refs in edges.values() {
        for name in refs {
            if !names.contains(name) {
                return Err(invalid("mise_task_dependency", format!("missing:{name}")));
            }
        }
    }
    if !names.contains(selected) {
        return Err(invalid("mise_task", format!("missing:{selected}")));
    }
    Ok(doc)
}

fn validate_assignment(
    a: &crate::toml_scan::TomlAssignment,
) -> Result<Option<Vec<String>>, MiseError> {
    if a.path.iter().any(|p| {
        matches!(
            p.as_str(),
            "includes" | "include" | "task_config" | "task_dir" | "task_dirs"
        )
    }) {
        return Err(invalid(
            "mise_config",
            "includes_and_file_tasks_unsupported",
        ));
    }
    if a.path.first().is_none_or(|p| p != "tasks") {
        return Ok(None);
    }
    if a.path.len() != 3 || !TASK_FIELDS.contains(&a.path[2].as_str()) {
        return Err(invalid("mise_task_field", a.path.join(".")));
    }
    if contains_template(&a.value) {
        return Err(invalid("mise_task", "dynamic_templates_unsupported"));
    }
    if !["depends", "depends_post", "wait_for", "alias"].contains(&a.path[2].as_str()) {
        return Ok(None);
    }
    let refs = strings(&a.value)?;
    for name in &refs {
        if !velnor_actions_contract::config::is_valid_mise_task_name(name) {
            return Err(invalid("mise_task_reference", name));
        }
    }
    Ok(Some(refs))
}

fn task_projection(doc: &crate::toml_scan::TomlDoc, bytes: &str) -> String {
    let lines: Vec<&str> = bytes.split_inclusive('\n').collect();
    let mut projection = String::new();
    for (i, (path, start)) in doc.sections.iter().enumerate() {
        if path.first().is_some_and(|p| p == "tasks") {
            let end = doc
                .sections
                .get(i + 1)
                .map_or(lines.len(), |(_, line)| *line as usize - 1);
            projection.extend(lines[*start as usize - 1..end].iter().copied());
            if !projection.ends_with('\n') {
                projection.push('\n');
            }
        }
    }
    projection
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

fn strings(value: &TomlValue) -> Result<Vec<String>, MiseError> {
    match value {
        TomlValue::Str(s) => Ok(vec![s.clone()]),
        TomlValue::Array(a) => a
            .iter()
            .map(|v| match v {
                TomlValue::Str(s) => Ok(s.clone()),
                _ => Err(invalid("mise_task_reference", "expected_string")),
            })
            .collect(),
        _ => Err(invalid("mise_task_reference", "expected_string_or_array")),
    }
}
fn contains_template(v: &TomlValue) -> bool {
    match v {
        TomlValue::Str(s) => s.contains("{{") || s.contains("{%"),
        TomlValue::Array(a) => a.iter().any(contains_template),
        _ => false,
    }
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
