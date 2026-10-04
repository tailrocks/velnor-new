//! Captured public Cargo closure; producer arguments carry data, never code.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{CompiledSourceHelper, ProposedTask, digest_b3};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::source_prep::SourceTransportAdmission;

// One hexadecimal argv must stay below Linux MAX_ARG_STRLEN (131072).
const MAX_JSON_BYTES: usize = 63 * 1024;
const MODE: &str = "complete-locked-workspace";

#[path = "source_producer_selection_payload.rs"]
mod payload;

#[path = "source_producer_descriptor_reconstruction.rs"]
mod reconstruction;

#[path = "source_producer_descriptor_identity.rs"]
mod identity;

pub(crate) use identity::RustSourceProjection;

#[cfg(test)]
#[path = "source_producer_identity_tests.rs"]
mod identity_tests;

/// Construction authority remains private; all evidence comes from admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RustSourceDescriptor {
    schema: u32,
    rust_version: String,
    target: String,
    roots: Vec<String>,
    manifests: Vec<(String, String)>,
    locks: Vec<(String, String)>,
    archives: Vec<(String, String, String)>,
    mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    selections: Vec<RustSourceSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RustSourceSelection {
    pub(crate) root: String,
    pub(crate) package: String,
    pub(crate) target: Option<String>,
    pub(crate) features: Vec<String>,
    pub(crate) default_features: bool,
}

impl RustSourceDescriptor {
    pub(super) fn source_projection(&self) -> Result<RustSourceProjection, OrchestratorError> {
        identity::projection(self)
    }

    pub(crate) fn selections(&self) -> Option<&[RustSourceSelection]> {
        (!self.selections.is_empty()).then_some(self.selections.as_slice())
    }

    /// Bounded hexadecimal literal safe for the compiled helper's sole argument.
    pub(crate) fn hex_json(&self) -> Result<String, OrchestratorError> {
        let bytes = self.json()?;
        let mut encoded = String::with_capacity(bytes.len() * 2);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
        Ok(encoded)
    }

    fn json(&self) -> Result<Vec<u8>, OrchestratorError> {
        let bytes = serde_json::to_vec(self).map_err(contract)?;
        if bytes.len() > MAX_JSON_BYTES {
            return Err(contract("rust_source_descriptor_too_large"));
        }
        Ok(bytes)
    }
}

/// Unsafe repository evidence disables optional publication; bad tool pins fail.
pub(crate) fn descriptor_for(
    root: &Path,
    roots: &[String],
    discovery: &Discovery,
    catalog: &ToolCatalog,
    target: &str,
) -> Result<Option<RustSourceDescriptor>, OrchestratorError> {
    let rust_version = catalog.version(PinnedTool::Rust);
    velnor_actions_mise::catalog::validate_exact_version("rust", rust_version).map_err(contract)?;
    if rust_version != velnor_actions_mise::catalog::RUST_VERSION
        || target != velnor_actions_mise::catalog::RUST_TARGET_TRIPLE
    {
        return Ok(None);
    }
    let Some(admission) = SourceTransportAdmission::new(root, roots) else {
        return Ok(None);
    };
    // The isolated producer never consumes checkout configuration. Reject it
    // rather than claim that captured locks reproduce its resolution effects.
    if !admission.configs().is_empty() {
        return Ok(None);
    }
    let Ok(manifests) = super::manifest::manifest_inputs(root, admission.roots(), discovery) else {
        return Ok(None);
    };
    let Some(archives) = archive_inputs(admission.locks()) else {
        return Ok(None);
    };
    if archives.is_empty() {
        return Ok(None);
    }
    let descriptor = RustSourceDescriptor {
        schema: 1,
        rust_version: rust_version.to_owned(),
        target: target.to_owned(),
        roots: admission.roots().to_vec(),
        manifests,
        locks: admission.locks().to_vec(),
        archives,
        mode: MODE.to_owned(),
        selections: Vec::new(),
    };
    if descriptor.json().is_err() {
        return Ok(None);
    }
    Ok(Some(descriptor))
}

/// Exact selected task ownership permits Cargo's native containing tree query.
pub(crate) fn descriptor_for_tasks(
    root: &Path,
    roots: &[String],
    discovery: &Discovery,
    catalog: &ToolCatalog,
    target: &str,
    tasks: &[&ProposedTask],
) -> Result<Option<RustSourceDescriptor>, OrchestratorError> {
    let Some(mut descriptor) = descriptor_for(root, roots, discovery, catalog, target)? else {
        return Ok(None);
    };
    let selections: Option<Vec<_>> = tasks
        .iter()
        .filter(|task| task.stack_id == "rust")
        .map(|task| selection(task, discovery, &descriptor))
        .collect();
    if let Some(mut selections) = selections
        && !selections.is_empty()
    {
        selections.sort();
        selections.dedup();
        descriptor.selections = selections;
        descriptor.mode = "native-tree-selected-containing".to_owned();
        if descriptor.json().is_err() {
            descriptor.selections.clear();
            descriptor.mode = MODE.to_owned();
        }
    }
    Ok(Some(descriptor))
}

fn selection(
    task: &ProposedTask,
    discovery: &Discovery,
    descriptor: &RustSourceDescriptor,
) -> Option<RustSourceSelection> {
    let mut matches = discovery.workspaces.iter().flat_map(|workspace| {
        workspace.record.packages.iter().filter_map(move |package| {
            (package.id == task.identity.unit_id).then_some((&workspace.record, package))
        })
    });
    let (workspace, package) = matches.next()?;
    if matches.next().is_some()
        || package.external
        || !package.in_workspace
        || !workspace.members.contains(&package.id)
        || package.manifest != task.identity.unit_path
        || !descriptor.roots.contains(&workspace.workspace_root)
        || !descriptor
            .manifests
            .iter()
            .any(|(path, _)| path == &package.manifest)
        || !safe_name(&package.name)
        || !payload::matches(task, package)
    {
        return None;
    }
    if !super::manifest::selected_package(
        &descriptor.manifests,
        &workspace.workspace_root,
        &package.name,
    )
    .ok()?
    {
        return None;
    }
    let target = match task.identity.target.as_str() {
        "host" => None,
        target if velnor_actions_contract::is_supported_target(target) => Some(target.to_owned()),
        _ => return None,
    };
    let default_features = task.identity.features == ["default"];
    let mut features = if default_features {
        Vec::new()
    } else {
        task.identity.features.clone()
    };
    if features.iter().any(|feature| !safe_name(feature)) {
        return None;
    }
    features.sort();
    features.dedup();
    Some(RustSourceSelection {
        root: workspace.workspace_root.clone(),
        package: package.name.clone(),
        target,
        features,
        default_features,
    })
}

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn archive_inputs(locks: &[(String, String)]) -> Option<Vec<(String, String, String)>> {
    let mut archives = BTreeMap::new();
    for (_, lock) in locks {
        let parsed: toml::Table = toml::from_str(lock).ok()?;
        for package in parsed.get("package")?.as_array()? {
            let table = package.as_table()?;
            let Some(source) = table.get("source") else {
                continue;
            };
            if !matches!(
                source.as_str(),
                Some(
                    "registry+https://github.com/rust-lang/crates.io-index"
                        | "sparse+https://index.crates.io/"
                )
            ) {
                return None;
            }
            let identity = (
                table.get("name")?.as_str()?.to_owned(),
                table.get("version")?.as_str()?.to_owned(),
            );
            let checksum = table.get("checksum")?.as_str()?.to_owned();
            if let Some(existing) = archives.insert(identity, checksum.clone())
                && existing != checksum
            {
                return None;
            }
        }
    }
    Some(
        archives
            .into_iter()
            .map(|((name, version), hash)| (name, version, hash))
            .collect(),
    )
}

/// Cache identity includes captured source evidence and compiled acquisition code.
pub(crate) fn source_identity(
    descriptor: &RustSourceDescriptor,
    helper: &CompiledSourceHelper,
) -> Result<String, OrchestratorError> {
    let mut evidence = identity::json(descriptor)?;
    evidence.push(0);
    evidence.extend_from_slice(helper.invocation().descriptor().source_sha256().as_bytes());
    Ok(format!(
        "velnor-v4-cargo-source-public-{}-{}",
        descriptor.target,
        digest_b3(&evidence)
    ))
}

fn contract(problem: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(name: &str, checksum: &str) -> String {
        format!(
            "version=4\n[[package]]\nname='{name}'\nversion='1.0.0'\n\
             source='registry+https://github.com/rust-lang/crates.io-index'\n\
             checksum='{checksum}'\n"
        )
    }

    #[test]
    fn archives_are_sorted_deduplicated_and_checksum_conflicts_rejected() {
        let locks = vec![
            ("a".to_owned(), lock("z", &"a".repeat(64))),
            ("b".to_owned(), lock("z", &"a".repeat(64))),
            ("c".to_owned(), lock("a", &"b".repeat(64))),
        ];
        let archives = archive_inputs(&locks).expect("qualified locks");
        assert_eq!(archives.len(), 2);
        assert_eq!(archives[0].0, "a");
        assert_eq!(archives[1].0, "z");
        assert!(
            archive_inputs(&[
                ("a".to_owned(), lock("z", &"a".repeat(64))),
                ("b".to_owned(), lock("z", &"b".repeat(64))),
            ])
            .is_none()
        );
    }

    #[test]
    fn descriptor_encoding_is_literal_and_bounded() {
        let mut descriptor = RustSourceDescriptor {
            schema: 1,
            rust_version: "1.98.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            roots: vec![String::new()],
            manifests: vec![("Cargo.toml".to_owned(), "$(private)\n'\"".to_owned())],
            locks: vec![(String::new(), lock("z", &"a".repeat(64)))],
            archives: vec![("z".to_owned(), "1.0.0".to_owned(), "a".repeat(64))],
            mode: MODE.to_owned(),
            selections: Vec::new(),
        };
        let encoded = descriptor.hex_json().expect("hexadecimal");
        assert_eq!(encoded.len(), descriptor.json().expect("json").len() * 2);
        assert!(encoded.bytes().all(|byte| byte.is_ascii_hexdigit()));
        descriptor.manifests[0].1 = "a".repeat(MAX_JSON_BYTES);
        assert!(descriptor.hex_json().is_err());
    }
}
