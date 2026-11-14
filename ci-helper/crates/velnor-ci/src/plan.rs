// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{AllTrigger, PackageName, RepoPath, RustSelection};
use velnor_rust::{CONFIG_PATH, root_slug};

use crate::error::CiError;

pub fn outputs(selections: &BTreeMap<RepoPath, RustSelection>) -> Result<String, CiError> {
    let mut owners = BTreeMap::new();
    let mut lines = Vec::new();
    for (root, selection) in selections {
        let slug = root_slug(root);
        if let Some(other) = owners.insert(slug.clone(), root) {
            return Err(CiError::new(format!(
                "the roots {other} and {root} both get the output name rust_{slug}; move one of them"
            )));
        }
        let (mode, packages, filter) = match selection {
            RustSelection::None => ("none", Vec::new(), String::new()),
            RustSelection::All(_) => ("all", Vec::new(), "all()".to_owned()),
            RustSelection::Some(packages) => {
                let names: Vec<&str> = packages.keys().map(PackageName::as_str).collect();
                let filter = names
                    .iter()
                    .map(|name| format!("package({name})"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                ("some", names, filter)
            }
        };
        lines.push(format!("rust_{slug}_mode={mode}\n"));
        lines.push(format!("rust_{slug}_packages={}\n", packages.join(" ")));
        lines.push(format!("rust_{slug}_test_filter={filter}\n"));
    }
    Ok(lines.concat())
}

#[must_use]
pub fn explain(
    selections: &BTreeMap<RepoPath, RustSelection>,
    unknown: &BTreeSet<RepoPath>,
) -> String {
    let mut lines = Vec::new();
    for (root, selection) in selections {
        let (header, packages) = match selection {
            RustSelection::None => ("none".to_owned(), None),
            RustSelection::Some(packages) => ("some".to_owned(), Some(packages)),
            RustSelection::All(trigger @ AllTrigger::MostPackages(packages)) => {
                (trigger.to_string(), Some(packages))
            }
            RustSelection::All(trigger) => (trigger.to_string(), None),
        };
        lines.push(format!("{root}: {header}\n"));
        for (package, reason) in packages.into_iter().flatten() {
            lines.push(format!("  {package} {reason}\n"));
        }
    }
    for path in unknown {
        if path.as_str().contains('*') {
            lines.push(format!(
                "unknown path: {path}\n  no exact rule can name a path with a *; use a glob of a folder above it that ends in /** in non_inputs or inputs in {CONFIG_PATH}\n"
            ));
            continue;
        }
        let quoted = toml_string(path.as_str());
        lines.push(format!(
            "unknown path: {path}\n  to select nothing, add this line to non_inputs in {CONFIG_PATH}:\n    {quoted},\n  to select packages, add this to {CONFIG_PATH}:\n    [[inputs]]\n    glob = {quoted}\n    packages = [\"<package>\"]\n"
        ));
    }
    lines.concat()
}

fn toml_string(text: &str) -> String {
    let escaped: String = text
        .chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            other => vec![other],
        })
        .collect();
    format!("\"{escaped}\"")
}
