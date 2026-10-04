//! Exact, contained manifest closure for a data-only public source producer.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[path = "source_producer_manifest_policy.rs"]
mod policy;
use policy::{as_table, dependency_manifest, relative, unqualified, validate_dependency};

#[path = "source_producer_manifest_captured.rs"]
mod captured;
pub(crate) use captured::{qualify_captured, selected_package};

use crate::OrchestratorError;
use crate::discover::{Discovery, workspace_manifest};
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

const MAX_MANIFESTS: usize = 1024;
const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;
pub(crate) fn manifest_inputs(
    root: &Path,
    roots: &[String],
    discovery: &Discovery,
) -> Result<Vec<(String, String)>, OrchestratorError> {
    if roots.is_empty() {
        return Err(unqualified("empty_roots"));
    }
    let mut seeds = BTreeSet::new();
    for selected in roots {
        relative(selected, true)?;
        let record = discovery
            .workspaces
            .iter()
            .find(|workspace| workspace.record.workspace_root == *selected)
            .ok_or_else(|| unqualified("workspace_inventory_missing"))?;
        seeds.insert(workspace_manifest(selected));
        for package in &record.record.packages {
            if !package.external {
                relative(&package.manifest, false)?;
                seeds.insert(package.manifest.clone());
            } else if package.in_workspace || package.id.starts_with("path+") {
                return Err(unqualified("external_local_package"));
            }
        }
        if record.record.members.iter().any(|id| {
            !record
                .record
                .packages
                .iter()
                .any(|package| &package.id == id && !package.external)
        }) {
            return Err(unqualified("workspace_member_missing"));
        }
    }
    Closure::new(
        root,
        roots.iter().map(|root| workspace_manifest(root)).collect(),
    )
    .collect(seeds)
    .and_then(|inputs| {
        qualify_captured(roots, &inputs)?;
        Ok(inputs)
    })
}
struct Closure<'a> {
    root: Option<&'a Path>,
    inputs: BTreeMap<String, String>,
    parsed: BTreeMap<String, toml::Table>,
    pending: BTreeSet<String>,
    bytes: usize,
    inventoried: BTreeSet<String>,
}
impl<'a> Closure<'a> {
    fn new(root: &'a Path, inventoried: BTreeSet<String>) -> Self {
        Self {
            root: Some(root),
            inputs: BTreeMap::new(),
            parsed: BTreeMap::new(),
            pending: BTreeSet::new(),
            bytes: 0,
            inventoried,
        }
    }
    fn collect(
        mut self,
        seeds: BTreeSet<String>,
    ) -> Result<Vec<(String, String)>, OrchestratorError> {
        self.pending = seeds;
        let mut qualified = BTreeSet::new();
        while let Some(manifest) = self.pending.pop_first() {
            if !qualified.insert(manifest.clone()) {
                continue;
            }
            let parsed = self.load(&manifest)?;
            let workspace = self.workspace(&manifest, &parsed)?;
            self.qualify(&manifest, &parsed, workspace.as_deref())?;
        }
        Ok(self.inputs.into_iter().collect())
    }
    fn load(&mut self, manifest: &str) -> Result<toml::Table, OrchestratorError> {
        if let Some(parsed) = self.parsed.get(manifest) {
            return Ok(parsed.clone());
        }
        relative(manifest, false)?;
        if Path::new(manifest)
            .file_name()
            .and_then(|name| name.to_str())
            != Some("Cargo.toml")
            || self.inputs.len() >= MAX_MANIFESTS
        {
            return Err(unqualified("manifest_limit_or_name"));
        }
        let root = self
            .root
            .ok_or_else(|| unqualified("captured_manifest_missing"))?;
        let text = match read_repo_file(root, manifest, MAX_REPO_FILE_BYTES)? {
            RepoRead::Text(text) => text,
            RepoRead::Absent => return Err(unqualified("manifest_missing")),
        };
        let parsed =
            toml::from_str::<toml::Table>(&text).map_err(|_| unqualified("malformed_manifest"))?;
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > MAX_MANIFEST_BYTES {
            return Err(unqualified("manifest_byte_limit"));
        }
        self.inputs.insert(manifest.to_owned(), text);
        self.parsed.insert(manifest.to_owned(), parsed.clone());
        self.pending.insert(manifest.to_owned());
        Ok(parsed)
    }
    fn workspace(
        &mut self,
        manifest: &str,
        parsed: &toml::Table,
    ) -> Result<Option<String>, OrchestratorError> {
        if parsed.contains_key("workspace") {
            return Ok(Some(manifest.to_owned()));
        }
        if let Some(value) = parsed
            .get("package")
            .and_then(toml::Value::as_table)
            .and_then(|package| package.get("workspace"))
        {
            let path = value
                .as_str()
                .ok_or_else(|| unqualified("malformed_workspace"))?;
            let workspace = dependency_manifest(manifest, path)?;
            if !self.load(&workspace)?.contains_key("workspace") {
                return Err(unqualified("workspace_table_missing"));
            }
            self.pending.insert(workspace.clone());
            return Ok(Some(workspace));
        }
        let mut parent = Path::new(manifest).parent().and_then(Path::parent);
        while let Some(directory) = parent {
            let candidate = directory.join("Cargo.toml").to_string_lossy().into_owned();
            let exists = if let Some(root) = self.root {
                !matches!(
                    read_repo_file(root, &candidate, MAX_REPO_FILE_BYTES)?,
                    RepoRead::Absent
                )
            } else {
                self.parsed.contains_key(&candidate)
            };
            if exists {
                if self.load(&candidate)?.contains_key("workspace") {
                    self.pending.insert(candidate.clone());
                    return Ok(Some(candidate));
                }
            }
            parent = directory.parent();
        }
        Ok(None)
    }
    fn qualify(
        &mut self,
        manifest: &str,
        table: &toml::Table,
        workspace: Option<&str>,
    ) -> Result<(), OrchestratorError> {
        self.dependencies(manifest, table, workspace)?;
        if let Some(targets) = table.get("target") {
            for target in as_table(targets)?.values() {
                self.dependencies(manifest, as_table(target)?, workspace)?;
            }
        }
        if let Some(value) = table.get("workspace") {
            let workspace = as_table(value)?;
            if let Some(members) = workspace.get("members") {
                let members = members
                    .as_array()
                    .ok_or_else(|| unqualified("malformed_members"))?;
                if members.iter().any(|member| member.as_str().is_none()) {
                    return Err(unqualified("workspace_members_uninventoried"));
                }
                for member in members {
                    let member = member
                        .as_str()
                        .ok_or_else(|| unqualified("malformed_member"))?;
                    if member.contains(['*', '?', '[', ']']) {
                        if !self.inventoried.contains(manifest) {
                            return Err(unqualified("workspace_members_uninventoried"));
                        }
                    } else {
                        self.pending.insert(dependency_manifest(manifest, member)?);
                    }
                }
            }
            self.dependencies(manifest, workspace, None)?;
        }
        if let Some(patches) = table.get("patch") {
            for (source, deps) in as_table(patches)? {
                if !matches!(
                    source.as_str(),
                    "crates-io"
                        | "https://github.com/rust-lang/crates.io-index"
                        | "https://index.crates.io/"
                ) {
                    return Err(unqualified("private_patch_source"));
                }
                self.dependency_table(manifest, as_table(deps)?, None)?;
            }
        }
        if let Some(replacements) = table.get("replace") {
            let replacements = as_table(replacements)?;
            if replacements
                .keys()
                .any(|key| key.contains(['/', '#', '\\']))
            {
                return Err(unqualified("private_replacement_source"));
            }
            self.dependency_table(manifest, replacements, None)?;
        }
        Ok(())
    }
    fn dependencies(
        &mut self,
        manifest: &str,
        table: &toml::Table,
        workspace: Option<&str>,
    ) -> Result<(), OrchestratorError> {
        for legacy in ["build_dependencies", "dev_dependencies"] {
            if table.contains_key(legacy) {
                return Err(unqualified("legacy_dependency_table"));
            }
        }
        for key in ["dependencies", "build-dependencies", "dev-dependencies"] {
            if let Some(deps) = table.get(key) {
                self.dependency_table(manifest, as_table(deps)?, workspace)?;
            }
        }
        Ok(())
    }
    fn dependency_table(
        &mut self,
        manifest: &str,
        table: &toml::Table,
        workspace: Option<&str>,
    ) -> Result<(), OrchestratorError> {
        for (name, dependency) in table {
            if let Some(version) = dependency.as_str() {
                if version.is_empty() {
                    return Err(unqualified("empty_dependency_version"));
                }
                continue;
            }
            let dependency = as_table(dependency)?;
            validate_dependency(dependency)?;
            if dependency.contains_key("workspace") {
                let owner = workspace.ok_or_else(|| unqualified("workspace_dependency_unbound"))?;
                let parsed = self.load(owner)?;
                let inherited = parsed
                    .get("workspace")
                    .and_then(toml::Value::as_table)
                    .and_then(|table| table.get("dependencies"))
                    .and_then(toml::Value::as_table)
                    .and_then(|deps| deps.get(name))
                    .ok_or_else(|| unqualified("workspace_dependency_missing"))?;
                let deps = [(name.clone(), inherited.clone())].into_iter().collect();
                self.dependency_table(owner, &deps, None)?;
            } else if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
                self.pending.insert(dependency_manifest(manifest, path)?);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "source_producer_manifest_tests.rs"]
mod tests;
