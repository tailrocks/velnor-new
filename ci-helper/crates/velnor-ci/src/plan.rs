// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    AllTrigger, OptionalSteps, PackageName, RepoPath, RustSelection, StepNeed, Unread, Weakening,
};
use velnor_rust::{CONFIG_PATH, root_slug};

use crate::error::CiError;

pub type Steps = BTreeMap<RepoPath, OptionalSteps>;

pub fn outputs(
    selections: &BTreeMap<RepoPath, RustSelection>,
    steps: &Steps,
) -> Result<String, CiError> {
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
        if let Some(steps) = steps.get(root) {
            lines.push(format!("rust_{slug}_doctests={}\n", flag(steps.doctests)));
            lines.push(format!("rust_{slug}_docs={}\n", flag(steps.docs)));
        }
    }
    Ok(lines.concat())
}

#[must_use]
pub fn weakened(all: &[Weakening], new: &[Weakening]) -> String {
    let mut lines = Vec::new();
    for weakening in all {
        let marker = if new.contains(weakening) {
            " (new in this change)"
        } else {
            ""
        };
        lines.push(format!(
            "check does less: {}: {}: {}{marker}\n",
            escape(weakening.file.as_str(), false),
            escape(&weakening.setting, false),
            escape(&weakening.effect, false),
        ));
    }
    lines.concat()
}

#[must_use]
pub fn warnings(new: &[Weakening]) -> String {
    let mut lines = Vec::new();
    for weakening in new {
        lines.push(format!(
            "::warning file={}::this pull request {} ({})\n",
            escape(weakening.file.as_str(), true),
            escape(&weakening.effect, false),
            escape(&weakening.setting, false),
        ));
    }
    lines.concat()
}

fn inert(text: &str) -> String {
    escape(text, false).replace(':', "%3A")
}

fn inert_reason(reason: &str) -> String {
    reason.split_once(':').map_or_else(
        || inert(reason),
        |(kind, rest)| format!("{kind}:{}", inert(rest)),
    )
}

fn escape(text: &str, property: bool) -> String {
    text.chars()
        .map(|c| match c {
            '%' => "%25".to_owned(),
            '\r' => "%0D".to_owned(),
            '\n' => "%0A".to_owned(),
            ':' if property => "%3A".to_owned(),
            ',' if property => "%2C".to_owned(),
            odd if odd.is_control() || (odd.is_whitespace() && odd != ' ') => {
                format!("%{:02X}", u32::from(odd))
            }
            other => other.to_string(),
        })
        .collect::<String>()
        .replace("##[", "#%23[")
}

#[must_use]
pub fn explain(
    selections: &BTreeMap<RepoPath, RustSelection>,
    steps: &Steps,
    unknown: &BTreeSet<RepoPath>,
    unread: &BTreeMap<RepoPath, Unread>,
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
        lines.push(format!(
            "{}: {}\n",
            inert(root.as_str()),
            escape(&header, false)
        ));
        for (package, reason) in packages.into_iter().flatten() {
            lines.push(format!(
                "  {} {}\n",
                inert(&package.to_string()),
                inert_reason(&reason.to_string())
            ));
        }
        let selected = !matches!(selection, RustSelection::None);
        for (step, need) in steps
            .get(root)
            .filter(|_| selected)
            .into_iter()
            .flat_map(|steps| [("doctests", steps.doctests), ("documentation", steps.docs)])
        {
            if let StepNeed::Skip(reason) = need {
                lines.push(format!("  left out: {step}: {reason}\n"));
            }
        }
    }
    for (manifest, cause) in unread {
        let (manifest, cause) = (
            escape(manifest.as_str(), false),
            escape(&cause.to_string(), false),
        );
        lines.push(format!(
            "unread package: {manifest}\n  {cause}, so Velnor cannot follow its dependencies and a change to any package selects every package that depends on it\n"
        ));
    }
    for path in unknown {
        let shown = escape(path.as_str(), false);
        if path.as_str().contains('*') {
            lines.push(format!(
                "unknown path: {shown}\n  no exact rule can name a path with a *; use a glob of a folder above it that ends in /** in non_inputs or inputs in {CONFIG_PATH}\n"
            ));
            continue;
        }
        let quoted = toml_string(&shown);
        lines.push(format!(
            "unknown path: {shown}\n  to select nothing, add this line to non_inputs in {CONFIG_PATH}:\n    {quoted},\n  to select packages, add this to {CONFIG_PATH}:\n    [[inputs]]\n    glob = {quoted}\n    packages = [\"<package>\"]\n"
        ));
    }
    lines.concat()
}

fn flag(need: StepNeed) -> &'static str {
    match need {
        StepNeed::Run => "run",
        StepNeed::Skip(_) => "skip",
    }
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
