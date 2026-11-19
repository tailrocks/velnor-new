// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use toml::Value;

use super::{DEP_SECTIONS, MemberWorkspace};

pub(super) fn path_dependencies(
    document: &Value,
    base: &Path,
    workspace: &MemberWorkspace,
) -> Vec<(String, PathBuf)> {
    fn collect(
        node: &Value,
        base: &Path,
        workspace: &MemberWorkspace,
        paths: &mut Vec<(String, PathBuf)>,
    ) {
        let Some(table) = node.as_table() else { return };
        for section in DEP_SECTIONS {
            for (alias, value) in table
                .get(*section)
                .and_then(Value::as_table)
                .into_iter()
                .flatten()
            {
                let Some(spec) = value.as_table() else {
                    continue;
                };
                let (spec, base) = if spec.get("workspace").and_then(Value::as_bool) == Some(true) {
                    let Some(inherited) = workspace
                        .document
                        .get("workspace")
                        .and_then(|v| v.get("dependencies"))
                        .and_then(|v| v.get(alias))
                        .and_then(Value::as_table)
                    else {
                        continue;
                    };
                    (inherited, workspace.root.as_path())
                } else {
                    (spec, base)
                };
                if let Some(path) = spec.get("path").and_then(Value::as_str) {
                    let name = spec.get("package").and_then(Value::as_str).unwrap_or(alias);
                    paths.push((name.to_owned(), base.join(path).join("Cargo.toml")));
                }
            }
        }
        for target in table
            .get("target")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
            .map(|(_, value)| value)
        {
            collect(target, base, workspace, paths);
        }
    }
    let mut paths = Vec::new();
    collect(document, base, workspace, &mut paths);
    paths
}
