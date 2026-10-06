//! Admission and ownership use the same manifest policy without filesystem reads.

use super::policy::{as_table, dependency_manifest, relative, unqualified};
use super::{BTreeMap, BTreeSet, Closure, OrchestratorError, Path, workspace_manifest};

const MAX_CAPTURE_BYTES: usize = 63 * 1024;

pub(crate) fn qualify_captured(
    roots: &[String],
    manifests: &[(String, String)],
) -> Result<(), OrchestratorError> {
    let closure = captured(roots, manifests)?;
    let seeds = closure.parsed.keys().cloned().collect();
    closure.collect(seeds)?;
    Ok(())
}

fn captured<'a>(
    roots: &[String],
    manifests: &[(String, String)],
) -> Result<Closure<'a>, OrchestratorError> {
    if roots.is_empty()
        || manifests.is_empty()
        || manifests.len() > super::MAX_MANIFESTS
        || !roots.windows(2).all(|pair| pair[0] < pair[1])
        || !manifests.windows(2).all(|pair| pair[0].0 < pair[1].0)
    {
        return Err(unqualified("captured_inputs_order_or_count"));
    }
    let mut closure = Closure {
        root: None,
        inputs: BTreeMap::new(),
        parsed: BTreeMap::new(),
        pending: BTreeSet::new(),
        bytes: 0,
        inventoried: roots.iter().map(|root| workspace_manifest(root)).collect(),
    };
    for root in roots {
        relative(root, true)?;
    }
    for (path, text) in manifests {
        relative(path, false)?;
        if Path::new(path).file_name().and_then(|name| name.to_str()) != Some("Cargo.toml") {
            return Err(unqualified("captured_manifest_name"));
        }
        closure.bytes = closure
            .bytes
            .saturating_add(path.len())
            .saturating_add(text.len());
        if closure.bytes > MAX_CAPTURE_BYTES {
            return Err(unqualified("captured_byte_limit"));
        }
        let parsed =
            toml::from_str::<toml::Table>(text).map_err(|_| unqualified("malformed_manifest"))?;
        closure.inputs.insert(path.clone(), text.clone());
        closure.parsed.insert(path.clone(), parsed);
    }
    for root in roots {
        if !closure.parsed.contains_key(&workspace_manifest(root)) {
            return Err(unqualified("captured_root_missing"));
        }
    }
    for (manifest, table) in &closure.parsed {
        if let Some(workspace) = table.get("workspace") {
            qualify_members(manifest, as_table(workspace)?, &closure.parsed)?;
        }
    }
    Ok(closure)
}

fn qualify_members(
    manifest: &str,
    workspace: &toml::Table,
    captured: &BTreeMap<String, toml::Table>,
) -> Result<(), OrchestratorError> {
    for field in ["members", "default-members", "exclude"] {
        let Some(value) = workspace.get(field) else {
            continue;
        };
        let members = value
            .as_array()
            .ok_or_else(|| unqualified("malformed_members"))?;
        for member in members {
            let member = member
                .as_str()
                .ok_or_else(|| unqualified("malformed_member"))?;
            // A tuple map cannot prove directory/glob expansion; reject ambiguity.
            if member.is_empty() || member.contains(['*', '?', '[', ']', '\0']) {
                return Err(unqualified("captured_member_pattern_opaque"));
            }
            let path = dependency_manifest(manifest, member)?;
            if field != "exclude" && !captured.contains_key(&path) {
                return Err(unqualified("captured_member_missing"));
            }
        }
    }
    Ok(())
}

pub(crate) fn selected_package(
    manifests: &[(String, String)],
    root: &str,
    package: &str,
) -> Result<bool, OrchestratorError> {
    qualify_captured(&[root.to_owned()], manifests)?;
    let mut closure = captured(&[root.to_owned()], manifests)?;
    let owner = workspace_manifest(root);
    let parsed = closure.load(&owner)?;
    let mut members = BTreeSet::from([owner.clone()]);
    if let Some(workspace) = parsed.get("workspace") {
        let workspace = as_table(workspace)?;
        if let Some(list) = workspace.get("members").and_then(toml::Value::as_array) {
            for member in list {
                let member = member
                    .as_str()
                    .ok_or_else(|| unqualified("malformed_member"))?;
                members.insert(dependency_manifest(&owner, member)?);
            }
        }
        if let Some(excludes) = workspace.get("exclude").and_then(toml::Value::as_array) {
            for exclude in excludes {
                let exclude = exclude
                    .as_str()
                    .ok_or_else(|| unqualified("malformed_member"))?;
                members.remove(&dependency_manifest(&owner, exclude)?);
            }
        }
    }
    let matches: Vec<_> = closure
        .parsed
        .iter()
        .filter(|(_, table)| {
            table
                .get("package")
                .and_then(toml::Value::as_table)
                .and_then(|package_table| package_table.get("name"))
                .and_then(toml::Value::as_str)
                == Some(package)
        })
        .map(|(path, _)| path.clone())
        .collect();
    let [path] = matches.as_slice() else {
        return Ok(false);
    };
    if !members.contains(path) {
        return Ok(false);
    }
    let table = closure.load(path)?;
    Ok(if path == &owner {
        true
    } else {
        closure.workspace(path, &table)?.as_deref() == Some(owner.as_str())
    })
}
