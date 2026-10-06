//! Pure resolution of explicitly qualified installation closures.
use super::invalid;
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract_config::config::{
    CheckPlatform, CheckSystemToolKind, HostContainerProfile, MiseCheck, QualifiedTool,
    validate_qualified_tools,
};
use velnor_actions_mise_core::MiseError;
use velnor_actions_mise_core::checks::fingerprint;

pub(super) struct ResolvedTools {
    pub(super) declarations: Vec<QualifiedTool>,
    pub(super) specs: Vec<String>,
    pub(super) fingerprint: String,
}
/// Resolve every selected ID from the explicit qualified registry.
pub(super) fn resolve(
    registry: &[QualifiedTool],
    roots: &[String],
    check: &MiseCheck,
) -> Result<ResolvedTools, MiseError> {
    let platform = check.runner.platform;
    validate_qualified_tools(registry, ".velnor/config.toml")
        .map_err(|e| invalid("qualified_tools", e.to_string()))?;
    let registry: BTreeMap<&str, &QualifiedTool> = registry
        .iter()
        .map(|tool| (tool.id.as_str(), tool))
        .collect();
    let mut declarations = Vec::new();
    let mut visited = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut selectors = BTreeMap::new();
    let roots: BTreeSet<&str> = roots.iter().map(String::as_str).collect();
    for id in roots {
        let tool = registry
            .get(id)
            .ok_or_else(|| invalid("qualified_tool_id", format!("undeclared:{id}")))?;
        visit(
            tool,
            &registry,
            platform,
            &mut visited,
            &mut active,
            &mut declarations,
        )?;
    }
    let mut executables = BTreeSet::new();
    let reserved = reserved_executables(check);
    for tool in &declarations {
        insert_selector(
            &mut selectors,
            velnor_actions_mise_core::checks::selector(tool),
        )?;
        let qualification = tool
            .platforms
            .iter()
            .find(|proof| proof.platform == platform)
            .ok_or_else(|| invalid("qualified_tool_platform", &tool.id))?;
        for executable in &qualification.executables {
            if reserved.contains(executable.name.as_str()) {
                return Err(invalid(
                    "qualified_tool_executable",
                    format!("runtime_owned_projected_name:{}", executable.name),
                ));
            }
            if !executables.insert(&executable.name) {
                return Err(invalid(
                    "qualified_tool_executable",
                    "duplicate_projected_name",
                ));
            }
        }
    }
    let specs: Vec<String> = selectors.into_values().collect();
    let fingerprint = fingerprint(&declarations, &specs)?;
    Ok(ResolvedTools {
        declarations,
        specs,
        fingerprint,
    })
}

fn reserved_executables(check: &MiseCheck) -> BTreeSet<&'static str> {
    let mut names = BTreeSet::from(["mise"]);
    match check.runner.container.as_ref() {
        Some(HostContainerProfile::Docker { .. }) => {
            names.insert("docker");
        }
        Some(HostContainerProfile::OrbStack { .. }) => {
            names.insert("docker");
            names.insert("orbctl");
        }
        None => {}
    }
    for tool in &check.system_tools {
        match tool.kind {
            CheckSystemToolKind::Swift => {
                names.insert("swift");
            }
            CheckSystemToolKind::Xcode => {
                names.insert("xcodebuild");
            }
        }
    }
    names
}
fn visit<'a>(
    tool: &'a QualifiedTool,
    registry: &BTreeMap<&str, &'a QualifiedTool>,
    platform: CheckPlatform,
    visited: &mut BTreeSet<String>,
    active: &mut BTreeSet<String>,
    output: &mut Vec<QualifiedTool>,
) -> Result<(), MiseError> {
    if visited.contains(&tool.id) {
        return Ok(());
    }
    if !active.insert(tool.id.clone()) {
        return Err(invalid("qualified_tools", "dependency_cycle"));
    }
    if !tool
        .platforms
        .iter()
        .any(|proof| proof.platform == platform)
    {
        return Err(invalid("qualified_tool_platform", &tool.id));
    }
    for id in &tool.depends_on {
        let dependency = registry
            .get(id.as_str())
            .ok_or_else(|| invalid("qualified_tool_dependency", id))?;
        visit(dependency, registry, platform, visited, active, output)?;
    }
    active.remove(&tool.id);
    visited.insert(tool.id.clone());
    output.push(tool.clone());
    Ok(())
}
fn insert_selector(
    selectors: &mut BTreeMap<String, String>,
    spec: String,
) -> Result<(), MiseError> {
    let (key, _) = spec
        .rsplit_once('@')
        .ok_or_else(|| invalid("qualified_tools", "nonexact_selector"))?;
    if let Some(existing) = selectors.get(key) {
        if existing != &spec {
            return Err(invalid("qualified_tools", "conflicting_selected_versions"));
        }
        return Err(invalid("qualified_tools", "duplicate_selected_backend"));
    }
    selectors.insert(key.to_owned(), spec);
    Ok(())
}

#[cfg(test)]
mod tests;
