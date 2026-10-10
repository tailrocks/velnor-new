//! Share repeated workflow commands, input mappings, and exact steps.

use std::collections::{BTreeMap, BTreeSet};

use crate::yaml::{AnchorName, Yaml, quote_scalar, render_yaml};

#[derive(Default)]
struct RepeatGroup {
    count: usize,
    total_bytes: usize,
    first_bytes: usize,
}

pub(super) fn share_repeated_workflow_nodes(mut node: Yaml) -> Yaml {
    let mut used = BTreeSet::new();
    collect_anchor_names(&node, &mut used);

    share_step_run_scalars(&mut node, &mut used);
    share_workflow_mappings(&mut node, &mut used);
    share_step_mappings(&mut node, &mut used);
    node
}

fn share_step_run_scalars(node: &mut Yaml, used: &mut BTreeSet<AnchorName>) {
    let mut counts = BTreeMap::new();
    visit_step_runs(node, &mut |value| {
        if let Yaml::Str(command) = value {
            *counts.entry(command.clone()).or_insert(0) += 1;
        }
    });
    let mut anchors = BTreeMap::new();
    replace_step_runs(node, &counts, used, &mut anchors, &mut 1);
}

fn visit_step_runs(node: &Yaml, visit: &mut impl FnMut(&Yaml)) {
    let Yaml::Map(root) = node else { return };
    for (key, jobs) in root {
        if key != "jobs" {
            continue;
        }
        let Yaml::Map(jobs) = jobs else { continue };
        for (_, job) in jobs {
            let Yaml::Map(job) = job else { continue };
            for (key, steps) in job {
                if key != "steps" {
                    continue;
                }
                let Yaml::Seq(steps) = steps else { continue };
                for step in steps {
                    if let Yaml::Map(step) = step {
                        for (key, value) in step {
                            if key == "run" {
                                visit(value);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn replace_step_runs(
    node: &mut Yaml,
    counts: &BTreeMap<String, usize>,
    used: &mut BTreeSet<AnchorName>,
    anchors: &mut BTreeMap<String, AnchorName>,
    next: &mut usize,
) {
    let Yaml::Map(root) = node else { return };
    for (key, jobs) in root {
        if key != "jobs" {
            continue;
        }
        let Yaml::Map(jobs) = jobs else { continue };
        for (_, job) in jobs {
            let Yaml::Map(job) = job else { continue };
            for (key, steps) in job {
                if key != "steps" {
                    continue;
                }
                let Yaml::Seq(steps) = steps else { continue };
                for step in steps {
                    let Yaml::Map(step) = step else { continue };
                    for (key, value) in step {
                        if key != "run" {
                            continue;
                        }
                        let Yaml::Str(command) = value else { continue };
                        let command = command.clone();
                        if let Some(existing) = anchors.get(&command) {
                            *value = Yaml::Alias(existing.clone());
                            continue;
                        }
                        let count = counts.get(&command).copied().unwrap_or_default();
                        if count < 2 {
                            continue;
                        }
                        let name = next_name(used, "r", next);
                        if !saves_repeated_node(
                            quote_scalar(&command).len().saturating_mul(count),
                            quote_scalar(&command).len(),
                            count,
                            &name,
                        ) {
                            continue;
                        }
                        used.insert(name.clone());
                        anchors.insert(command.clone(), name.clone());
                        *value = Yaml::AnchoredScalar {
                            name,
                            value: command,
                        };
                    }
                }
            }
        }
    }
}

fn share_workflow_mappings(node: &mut Yaml, used: &mut BTreeSet<AnchorName>) {
    let anchors = anchor_values(node);
    let mut groups = BTreeMap::new();
    visit_shareable_mappings(node, &mut |mapping| {
        note_repeat(&mut groups, mapping, &anchors);
    });
    replace_shareable_mappings(node, &groups, &anchors, used);
}

fn visit_shareable_mappings(node: &Yaml, visit: &mut impl FnMut(&Yaml)) {
    let Yaml::Map(root) = node else { return };
    for (key, jobs) in root {
        if key != "jobs" {
            continue;
        }
        let Yaml::Map(jobs) = jobs else { continue };
        for (_, job) in jobs {
            let Yaml::Map(job) = job else { continue };
            for (key, value) in job {
                if key == "env" {
                    visit(value);
                } else if key == "steps" {
                    let Yaml::Seq(steps) = value else { continue };
                    for step in steps {
                        let Yaml::Map(step) = step else { continue };
                        for (key, value) in step {
                            if matches!(key.as_str(), "env" | "with") {
                                visit(value);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn replace_shareable_mappings(
    node: &mut Yaml,
    groups: &BTreeMap<String, RepeatGroup>,
    anchors: &BTreeMap<AnchorName, Yaml>,
    used: &mut BTreeSet<AnchorName>,
) {
    let Yaml::Map(root) = node else { return };
    let mut named = BTreeMap::new();
    let mut next = 1;
    for (key, jobs) in root {
        if key != "jobs" {
            continue;
        }
        let Yaml::Map(jobs) = jobs else { continue };
        for (_, job) in jobs {
            let Yaml::Map(job) = job else { continue };
            for (key, value) in job {
                if key == "env" {
                    replace_mapping(value, groups, anchors, used, &mut named, "m", &mut next);
                } else if key == "steps" {
                    let Yaml::Seq(steps) = value else { continue };
                    for step in steps {
                        let Yaml::Map(step) = step else { continue };
                        for (key, value) in step {
                            if matches!(key.as_str(), "env" | "with") {
                                replace_mapping(
                                    value, groups, anchors, used, &mut named, "m", &mut next,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn share_step_mappings(node: &mut Yaml, used: &mut BTreeSet<AnchorName>) {
    let anchors = anchor_values(node);
    let mut groups = BTreeMap::new();
    visit_steps(node, &mut |step| note_repeat(&mut groups, step, &anchors));
    replace_steps(node, &groups, &anchors, used);
}

fn visit_steps(node: &Yaml, visit: &mut impl FnMut(&Yaml)) {
    let Yaml::Map(root) = node else { return };
    for (key, jobs) in root {
        if key != "jobs" {
            continue;
        }
        let Yaml::Map(jobs) = jobs else { continue };
        for (_, job) in jobs {
            let Yaml::Map(job) = job else { continue };
            for (key, steps) in job {
                if key != "steps" {
                    continue;
                }
                let Yaml::Seq(steps) = steps else { continue };
                for step in steps {
                    if matches!(step, Yaml::Map(_)) {
                        visit(step);
                    }
                }
            }
        }
    }
}

fn replace_steps(
    node: &mut Yaml,
    groups: &BTreeMap<String, RepeatGroup>,
    anchors: &BTreeMap<AnchorName, Yaml>,
    used: &mut BTreeSet<AnchorName>,
) {
    let Yaml::Map(root) = node else { return };
    let mut named = BTreeMap::new();
    let mut next = 1;
    for (key, jobs) in root {
        if key != "jobs" {
            continue;
        }
        let Yaml::Map(jobs) = jobs else { continue };
        for (_, job) in jobs {
            let Yaml::Map(job) = job else { continue };
            for (key, steps) in job {
                if key != "steps" {
                    continue;
                }
                let Yaml::Seq(steps) = steps else { continue };
                for step in steps {
                    replace_mapping(step, groups, anchors, used, &mut named, "s", &mut next);
                }
            }
        }
    }
}

fn note_repeat(
    groups: &mut BTreeMap<String, RepeatGroup>,
    value: &Yaml,
    anchors: &BTreeMap<AnchorName, Yaml>,
) {
    let Yaml::Map(_) = value else { return };
    let Some(signature) = canonical_signature(value, anchors) else {
        return;
    };
    let bytes = render_yaml(value).len();
    let group = groups.entry(signature).or_default();
    if group.count == 0 {
        group.first_bytes = bytes;
    }
    group.count += 1;
    group.total_bytes = group.total_bytes.saturating_add(bytes);
}

fn replace_mapping(
    value: &mut Yaml,
    groups: &BTreeMap<String, RepeatGroup>,
    anchors: &BTreeMap<AnchorName, Yaml>,
    used: &mut BTreeSet<AnchorName>,
    named: &mut BTreeMap<String, AnchorName>,
    prefix: &str,
    next: &mut usize,
) {
    if !matches!(value, Yaml::Map(_)) {
        return;
    }
    let Some(signature) = canonical_signature(value, anchors) else {
        return;
    };
    if let Some(name) = named.get(&signature) {
        *value = Yaml::Alias(name.clone());
        return;
    }
    let Some(group) = groups.get(&signature) else {
        return;
    };
    if group.count < 2 {
        return;
    }
    let name = next_name(used, prefix, next);
    if !saves_repeated_node(group.total_bytes, group.first_bytes, group.count, &name) {
        return;
    }
    let Yaml::Map(entries) = value else { return };
    let entries = std::mem::take(entries);
    used.insert(name.clone());
    named.insert(signature, name.clone());
    *value = Yaml::AnchoredMap { name, entries };
}

fn canonical_signature(value: &Yaml, anchors: &BTreeMap<AnchorName, Yaml>) -> Option<String> {
    let expanded = expand_anchors(value, anchors, &mut BTreeSet::new())?;
    Some(render_yaml(&expanded))
}

fn expand_anchors(
    value: &Yaml,
    anchors: &BTreeMap<AnchorName, Yaml>,
    resolving: &mut BTreeSet<AnchorName>,
) -> Option<Yaml> {
    Some(match value {
        Yaml::AnchoredScalar { value, .. } => Yaml::Str(value.clone()),
        Yaml::AnchoredMap { entries, .. } | Yaml::Map(entries) => Yaml::Map(
            entries
                .iter()
                .map(|(key, value)| Some((key.clone(), expand_anchors(value, anchors, resolving)?)))
                .collect::<Option<Vec<_>>>()?,
        ),
        Yaml::Alias(name) => {
            if !resolving.insert(name.clone()) {
                return None;
            }
            let expanded = expand_anchors(anchors.get(name)?, anchors, resolving);
            resolving.remove(name);
            expanded?
        }
        Yaml::Seq(items) => Yaml::Seq(
            items
                .iter()
                .map(|item| expand_anchors(item, anchors, resolving))
                .collect::<Option<Vec<_>>>()?,
        ),
        other => other.clone(),
    })
}

fn anchor_values(node: &Yaml) -> BTreeMap<AnchorName, Yaml> {
    let mut anchors = BTreeMap::new();
    collect_anchor_values(node, &mut anchors);
    anchors
}

fn collect_anchor_values(node: &Yaml, anchors: &mut BTreeMap<AnchorName, Yaml>) {
    match node {
        Yaml::AnchoredScalar { name, value } => {
            anchors.insert(name.clone(), Yaml::Str(value.clone()));
        }
        Yaml::AnchoredMap { name, entries } => {
            anchors.insert(name.clone(), Yaml::Map(entries.clone()));
            for (_, value) in entries {
                collect_anchor_values(value, anchors);
            }
        }
        Yaml::Map(entries) => {
            for (_, value) in entries {
                collect_anchor_values(value, anchors);
            }
        }
        Yaml::Seq(items) => {
            for value in items {
                collect_anchor_values(value, anchors);
            }
        }
        Yaml::Null
        | Yaml::Str(_)
        | Yaml::Bool(_)
        | Yaml::Int(_)
        | Yaml::Flow(_)
        | Yaml::Quoted(_)
        | Yaml::Annotated { .. }
        | Yaml::Alias(_) => {}
    }
}

fn saves_repeated_node(
    total_bytes: usize,
    first_bytes: usize,
    count: usize,
    name: &AnchorName,
) -> bool {
    let name_length = name.as_str().len();
    total_bytes
        > first_bytes
            .saturating_add(name_length + 2)
            .saturating_add((count - 1).saturating_mul(name_length + 1))
}

fn next_name(used: &BTreeSet<AnchorName>, prefix: &str, next: &mut usize) -> AnchorName {
    loop {
        let candidate = format!("{prefix}{}", *next);
        *next = next.saturating_add(1);
        if let Some(name) = AnchorName::new(candidate)
            && !used.contains(&name)
        {
            return name;
        }
    }
}

fn collect_anchor_names(node: &Yaml, names: &mut BTreeSet<AnchorName>) {
    match node {
        Yaml::AnchoredScalar { name, .. } | Yaml::Alias(name) => {
            names.insert(name.clone());
        }
        Yaml::Map(entries) => {
            for (_, value) in entries {
                collect_anchor_names(value, names);
            }
        }
        Yaml::AnchoredMap { name, entries } => {
            names.insert(name.clone());
            for (_, value) in entries {
                collect_anchor_names(value, names);
            }
        }
        Yaml::Seq(items) => {
            for value in items {
                collect_anchor_names(value, names);
            }
        }
        Yaml::Null
        | Yaml::Str(_)
        | Yaml::Bool(_)
        | Yaml::Int(_)
        | Yaml::Flow(_)
        | Yaml::Quoted(_)
        | Yaml::Annotated { .. } => {}
    }
}

#[cfg(test)]
#[path = "yaml_share_tests.rs"]
mod tests;
