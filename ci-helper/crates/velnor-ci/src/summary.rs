// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{
    AllTrigger, OptionalSteps, PackageName, RepoPath, RustMember, RustRoot, RustSelection,
    SelectionReason, StepNeed,
};

use crate::plan::Steps;

fn cell(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() || " _-./:+@=,()'".contains(c) {
                c
            } else {
                '?'
            }
        })
        .collect()
}

fn lanes(member: &RustMember, steps: Option<&OptionalSteps>) -> String {
    let mut lanes = vec!["lint"];
    if member.has_test_target() {
        lanes.push("test");
    }
    if let Some(steps) = steps {
        if steps.doctests == StepNeed::Run && member.targets.iter().any(|target| target.doctest) {
            lanes.push("doctests");
        }
        if steps.docs == StepNeed::Run {
            lanes.push("docs");
        }
    }
    lanes.join(", ")
}

fn rows(
    root: &RustRoot,
    reasons: &BTreeMap<PackageName, String>,
    only_listed: bool,
    steps: Option<&OptionalSteps>,
) -> Vec<String> {
    root.members
        .iter()
        .filter(|member| !only_listed || reasons.contains_key(&member.name))
        .map(|member| {
            let reason = reasons
                .get(&member.name)
                .map_or("every package runs", String::as_str);
            format!(
                "| {} | {} | {} |",
                cell(member.name.as_str()),
                cell(reason),
                lanes(member, steps)
            )
        })
        .collect()
}

fn reasons(packages: &BTreeMap<PackageName, SelectionReason>) -> BTreeMap<PackageName, String> {
    packages
        .iter()
        .map(|(package, reason)| (package.clone(), reason.to_string()))
        .collect()
}

#[must_use]
pub fn summary(
    roots: &[RustRoot],
    selections: &BTreeMap<RepoPath, RustSelection>,
    steps: &Steps,
) -> String {
    let mut lines = Vec::new();
    for root in roots {
        let name = cell(root.manifest.as_str());
        let (head, known, only_listed) = match selections.get(&root.manifest) {
            Some(RustSelection::Some(packages)) => {
                (format!("{name}: some packages"), reasons(packages), true)
            }
            Some(RustSelection::All(trigger @ AllTrigger::MostPackages(packages))) => (
                format!("**{}** ({name})", cell(&trigger.to_string())),
                reasons(packages),
                false,
            ),
            Some(RustSelection::All(trigger)) => (
                format!("**{}** ({name})", cell(&trigger.to_string())),
                BTreeMap::new(),
                false,
            ),
            Some(RustSelection::None) | None => {
                lines.push(format!("{name}: no package selected\n"));
                continue;
            }
        };
        lines.push(head);
        lines.push(String::new());
        lines.push("| Package | Reason | Lanes |".to_owned());
        lines.push("|---|---|---|".to_owned());
        lines.extend(rows(root, &known, only_listed, steps.get(&root.manifest)));
        lines.push(String::new());
    }
    lines.join("\n") + "\n"
}
