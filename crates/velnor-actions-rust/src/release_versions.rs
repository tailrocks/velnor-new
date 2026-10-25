use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::package_release_policy::{
    CargoVersionBinding, MAX_CARGO_VERSION_INPUT_BYTES, MAX_CARGO_VERSION_MANIFEST_BYTES,
};
use velnor_actions_contract::{ContractError, PackageReleasePlan, PackageReleasePolicy};
pub fn resolve_release_policy(
    policy: &PackageReleasePolicy,
    manifests: &BTreeMap<String, Vec<u8>>,
) -> Result<PackageReleasePlan, ContractError> {
    policy.validate()?;
    let expected = policy.manifest_paths().into_iter().collect::<BTreeSet<_>>();
    if manifests
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(invalid("manifest_closure_mismatch"));
    }
    let mut total = 0usize;
    let mut parsed = BTreeMap::new();
    for (path, bytes) in manifests {
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("manifest_size"))?;
        if bytes.is_empty()
            || bytes.len() > MAX_CARGO_VERSION_MANIFEST_BYTES
            || total > MAX_CARGO_VERSION_INPUT_BYTES
        {
            return Err(invalid("manifest_size"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("manifest_utf8"))?;
        let value: toml::Value = toml::from_str(text).map_err(|_| invalid("manifest_toml"))?;
        parsed.insert(path.as_str(), value);
    }
    let mut versions = BTreeMap::new();
    for binding in &policy.version.bindings {
        versions.insert(binding.package.as_str(), package_version(binding, &parsed)?);
    }
    let base = versions
        .get(policy.version.version_package.as_str())
        .ok_or_else(|| invalid("version_owner_missing"))?;
    if !velnor_actions_contract::is_exact_semver(base)
        || versions.values().any(|version| version != base)
    {
        return Err(invalid("shared_numeric_version_mismatch"));
    }
    policy.resolve((*base).clone())
}
pub(crate) fn package_version(
    binding: &CargoVersionBinding,
    manifests: &BTreeMap<&str, toml::Value>,
) -> Result<String, ContractError> {
    let manifest = manifests
        .get(binding.manifest_path.as_str())
        .ok_or_else(|| invalid("manifest_missing"))?;
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid("package_table_missing"))?;
    if package.get("name").and_then(toml::Value::as_str) != Some(binding.package.as_str()) {
        return Err(invalid("package_name_mismatch"));
    }
    if let Some(workspace) = package.get("workspace") {
        let workspace = workspace
            .as_str()
            .ok_or_else(|| invalid("workspace_path_type"))?;
        if explicit_workspace_path(&binding.manifest_path, workspace)?
            != binding.workspace_manifest_path
        {
            return Err(invalid("workspace_path_mismatch"));
        }
    }
    match package.get("version") {
        Some(toml::Value::String(version)) => Ok(version.clone()),
        Some(toml::Value::Table(table))
            if table.len() == 1
                && table.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
        {
            manifests
                .get(binding.workspace_manifest_path.as_str())
                .and_then(|workspace| workspace.get("workspace"))
                .and_then(|workspace| workspace.get("package"))
                .and_then(|package| package.get("version"))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| invalid("workspace_version_missing"))
        }
        _ => Err(invalid("package_version_declaration")),
    }
}
fn explicit_workspace_path(manifest: &str, workspace: &str) -> Result<String, ContractError> {
    if workspace.is_empty() || workspace.starts_with('/') || workspace.contains('\\') {
        return Err(invalid("workspace_path"));
    }
    let mut parts = manifest.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in workspace.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| invalid("workspace_path_escape"))?;
            }
            value => parts.push(value),
        }
    }
    parts.push("Cargo.toml");
    Ok(parts.join("/"))
}
fn invalid(reason: &'static str) -> ContractError {
    ContractError::identity("cargo_release_version", reason)
}
