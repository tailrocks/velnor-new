// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{AllTrigger, PackageName, RepoPath, RustSelection};
use velnor_rust::root_slug;

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
pub fn explain(selections: &BTreeMap<RepoPath, RustSelection>) -> String {
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
    lines.concat()
}
