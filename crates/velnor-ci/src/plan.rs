// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    AllTrigger, OptionalSteps, PackageName, RepoPath, RootSlug, RustSelection, StepNeed, Unread,
    Weakening, group_output_name,
};
use velnor_rust::{CONFIG_PATH, GroupPlan, PolicyRuns};

use crate::error::CiError;
use crate::escape::{escape, inert};

const WORKSPACE: &str = "--workspace";

pub type Steps = BTreeMap<RepoPath, OptionalSteps>;

pub fn outputs(
    selections: &BTreeMap<RepoPath, RustSelection>,
    steps: &Steps,
    policy: PolicyRuns,
) -> Result<String, CiError> {
    let mut owners = BTreeMap::new();
    let mut lines = Vec::new();
    for (root, selection) in selections {
        let slug = RootSlug::of(root);
        if let Some(other) = owners.insert(slug.clone(), root) {
            return Err(CiError::new(format!(
                "the roots {other} and {root} both get the output name rust_{slug}; move one of them"
            )));
        }
        let (mode, packages, filter, package_args) = match selection {
            RustSelection::None => ("none", Vec::new(), String::new(), String::new()),
            RustSelection::All(_) => ("all", Vec::new(), "all()".to_owned(), WORKSPACE.to_owned()),
            RustSelection::Some(packages) => {
                let names: Vec<&str> = packages.keys().map(PackageName::as_str).collect();
                let filter = names
                    .iter()
                    .map(|name| format!("package({})", inert(name)))
                    .collect::<Vec<_>>()
                    .join(" | ");
                let package_args = package_args(&names);
                ("some", names, filter, package_args)
            }
        };
        lines.push(format!("rust_{slug}_mode={mode}\n"));
        lines.push(format!("rust_{slug}_packages={}\n", packages.join(" ")));
        lines.push(format!("rust_{slug}_test_filter={filter}\n"));
        lines.push(format!("rust_{slug}_package_args={package_args}\n"));
        if let Some(steps) = steps.get(root) {
            lines.push(format!("rust_{slug}_doctests={}\n", flag(steps.doctests)));
            lines.push(format!(
                "rust_{slug}_doctests_known={}\n",
                steps.doctests_known
            ));
            lines.push(format!("rust_{slug}_docs={}\n", flag(steps.docs)));
        }
    }
    lines.push(format!("rust_deny={}\n", lane(policy.deny)));
    lines.push(format!("rust_machete={}\n", lane(policy.machete)));
    Ok(lines.concat())
}

#[must_use]
pub fn group_outputs(groups: &[GroupPlan]) -> String {
    groups
        .iter()
        .map(|group| {
            format!(
                "{}={}\n",
                group_output_name(&RootSlug::of(&group.root), &group.name),
                lane(group.decision.run)
            )
        })
        .collect::<Vec<String>>()
        .concat()
}

#[must_use]
pub fn group_explanation(groups: &[GroupPlan]) -> String {
    groups
        .iter()
        .map(|group| {
            format!(
                "test group {} ({}): {}: {}\n",
                inert(group.name.as_str()),
                inert(group.root.as_str()),
                lane(group.decision.run),
                escape(&group.decision.reason, false)
            )
        })
        .collect::<Vec<String>>()
        .concat()
}

#[must_use]
pub fn tests_outputs(expected: &BTreeMap<RepoPath, bool>) -> String {
    expected
        .iter()
        .map(|(root, run)| {
            format!(
                "rust_{}_tests={}\n",
                RootSlug::of(root),
                if *run { "run" } else { "skip" }
            )
        })
        .collect::<Vec<String>>()
        .concat()
}

fn package_args(names: &[&str]) -> String {
    if names.iter().all(|name| cargo_name(name)) {
        names
            .iter()
            .map(|name| format!("-p {name}"))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        WORKSPACE.to_owned()
    }
}

fn shell_inert(c: char) -> bool {
    !c.is_ascii() && !c.is_whitespace() && !c.is_control()
}

fn cargo_name(name: &str) -> bool {
    !name.starts_with('-')
        && !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || shell_inert(c))
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

fn inert_reason(reason: &str) -> String {
    reason.split_once(':').map_or_else(
        || inert(reason),
        |(kind, rest)| format!("{kind}:{}", inert(rest)),
    )
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
        let quoted = toml_string(path.as_str());
        lines.push(format!(
            "unknown path: {shown}\n  to select nothing, add this line to non_inputs in {CONFIG_PATH}:\n    {quoted},\n  to select packages, add this to {CONFIG_PATH}:\n    [[inputs]]\n    glob = {quoted}\n    packages = [\"<package>\"]\n"
        ));
    }
    lines.concat()
}

fn lane(run: bool) -> &'static str {
    if run { "run" } else { "skip" }
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
        .map(|c| match c {
            '"' => "\\\"".to_owned(),
            '\\' => "\\\\".to_owned(),
            odd if odd.is_control() || (odd.is_whitespace() && odd != ' ') => {
                format!("\\u{:04X}", u32::from(odd))
            }
            other => other.to_string(),
        })
        .collect::<String>()
        .replace("##[", "#\\u0023[");
    format!("\"{escaped}\"")
}
