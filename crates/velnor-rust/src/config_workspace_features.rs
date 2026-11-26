// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{RepoView, RustCiConfig, RustMember, RustRoot};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};
use crate::harness::implicit_features;
use crate::toml_file::table;

/// Refuses a `workspace_features` name that no package of some root defines: cargo fails the
/// workspace-wide command of that root with it, so the repository would only learn at run time.
/// A name with a slash (`package/feature`, `dependency/feature`) is passed to cargo as written.
pub(crate) fn check(
    view: &RepoView,
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
    for root in rust_roots {
        let defined: BTreeSet<String> = root
            .members
            .iter()
            .flat_map(|member| defined_features(view, member))
            .collect();
        let missing = config
            .workspace_features
            .names
            .iter()
            .find(|name| !name.contains('/') && !defined.contains(*name));
        if let Some(feature) = missing {
            return Err(DetectError::invalid(
                &CONFIG_PATH,
                Reason::UnknownWorkspaceFeature {
                    root: root
                        .manifest
                        .parent()
                        .map_or_else(|| ".".to_owned(), |folder| folder.to_string()),
                    feature: feature.clone(),
                },
            ));
        }
    }
    Ok(())
}

fn defined_features(view: &RepoView, member: &RustMember) -> BTreeSet<String> {
    let Some(manifest) = view
        .files
        .get(&member.manifest)
        .and_then(|bytes| table(bytes).ok())
    else {
        return BTreeSet::new();
    };
    manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(toml::Table::keys)
        .cloned()
        .chain(implicit_features(&manifest))
        .collect()
}
