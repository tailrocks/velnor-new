//! Share repeated workflow commands, input mappings, and exact steps.

use std::collections::{BTreeMap, BTreeSet};

use crate::yaml::{AnchorName, Yaml, quote_scalar};

#[path = "yaml_share_mappings.rs"]
mod mappings;

pub(super) fn share_repeated_workflow_nodes(mut node: Yaml) -> Yaml {
    let mut used = BTreeSet::new();
    collect_anchor_names(&node, &mut used);

    share_step_run_scalars(&mut node, &mut used);
    mappings::share_workflow_mappings(&mut node, &mut used);
    mappings::share_step_mappings(&mut node, &mut used);
    prune_unreferenced_anchors(&mut node);
    node
}

fn prune_unreferenced_anchors(node: &mut Yaml) {
    let mut aliases = BTreeMap::new();
    collect_alias_uses(node, &mut aliases);
    remove_unreferenced_anchors(node, &aliases);
}

fn collect_alias_uses(node: &Yaml, aliases: &mut BTreeMap<AnchorName, usize>) {
    match node {
        Yaml::Alias(name) => *aliases.entry(name.clone()).or_default() += 1,
        Yaml::AnchoredMap { entries, .. } | Yaml::Map(entries) => {
            for (_, value) in entries {
                collect_alias_uses(value, aliases);
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                collect_alias_uses(item, aliases);
            }
        }
        Yaml::AnchoredScalar { .. }
        | Yaml::Null
        | Yaml::Str(_)
        | Yaml::Bool(_)
        | Yaml::Int(_)
        | Yaml::Flow(_)
        | Yaml::FlowMap(_)
        | Yaml::Quoted(_)
        | Yaml::Annotated { .. } => {}
    }
}

fn remove_unreferenced_anchors(node: &mut Yaml, aliases: &BTreeMap<AnchorName, usize>) {
    match node {
        Yaml::AnchoredScalar { name, value }
            if aliases.get(name).copied().unwrap_or_default() == 0 =>
        {
            *node = Yaml::Str(std::mem::take(value));
        }
        Yaml::AnchoredMap { name, entries }
            if aliases.get(name).copied().unwrap_or_default() == 0 =>
        {
            *node = Yaml::Map(std::mem::take(entries));
            remove_unreferenced_anchors(node, aliases);
        }
        Yaml::AnchoredMap { entries, .. } | Yaml::Map(entries) => {
            for (_, value) in entries {
                remove_unreferenced_anchors(value, aliases);
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                remove_unreferenced_anchors(item, aliases);
            }
        }
        Yaml::Alias(_)
        | Yaml::AnchoredScalar { .. }
        | Yaml::Null
        | Yaml::Str(_)
        | Yaml::Bool(_)
        | Yaml::Int(_)
        | Yaml::Flow(_)
        | Yaml::FlowMap(_)
        | Yaml::Quoted(_)
        | Yaml::Annotated { .. } => {}
    }
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
        | Yaml::FlowMap(_)
        | Yaml::Quoted(_)
        | Yaml::Annotated { .. } => {}
    }
}

#[cfg(test)]
#[path = "yaml_share_tests.rs"]
mod tests;
