//! Workflow-only, equivalence-checked sharing of repeated scalars and mappings.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    RenderError,
    yaml::{AnchorName, Yaml, quote_scalar},
};

#[path = "yaml_share_mappings.rs"]
mod mappings;
#[path = "yaml_share_prune.rs"]
mod prune;
#[path = "yaml_share_validation.rs"]
mod validation;

/// Over-cap workflow fallback only; action manifests never call this projection.
pub(super) fn share_workflow_nodes(node: Yaml) -> Result<Yaml, RenderError> {
    let original = validation::expand(&node)?.value;
    let mut shared = share_step_run_scalars(node);
    let mut used = BTreeSet::new();
    collect_anchor_names(&shared, &mut used);
    mappings::share(&mut shared, mappings::Mode::Fields, &mut used)?;
    mappings::share(&mut shared, mappings::Mode::Steps, &mut used)?;
    if validation::expand(&shared)?.value != original {
        return Err(validation::invalid("projection_changed"));
    }
    prune::prune(&mut shared);
    if validation::expand(&shared)?.value != original {
        return Err(validation::invalid("projection_changed"));
    }
    Ok(shared)
}

pub(super) fn share_step_run_scalars(mut node: Yaml) -> Yaml {
    let mut counts = BTreeMap::new();
    visit_step_runs(&node, &mut |value| {
        if let Yaml::Str(command) = value {
            *counts.entry(command.clone()).or_insert(0) += 1;
        }
    });
    if counts.is_empty() {
        return node;
    }
    let mut used = BTreeSet::new();
    collect_anchor_names(&node, &mut used);
    replace_step_runs(&mut node, &counts, &mut used, &mut BTreeMap::new(), &mut 1);
    node
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
                    if let Yaml::Map(step) | Yaml::AnchoredMap { entries: step, .. } = step {
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
                    let (Yaml::Map(step) | Yaml::AnchoredMap { entries: step, .. }) = step else {
                        continue;
                    };
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
                        if !saves_bytes(&command, count, &name) {
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

fn saves_bytes(command: &str, count: usize, name: &AnchorName) -> bool {
    let scalar = quote_scalar(command).len();
    let name_length = name.as_str().len();
    scalar.saturating_mul(count)
        > scalar
            .saturating_add(name_length + 2)
            .saturating_add((count - 1).saturating_mul(name_length + 1))
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
        Yaml::Seq(items) => {
            for value in items {
                collect_anchor_names(value, names);
            }
        }
        Yaml::AnchoredMap { name, entries } => {
            names.insert(name.clone());
            for (_, value) in entries {
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

#[cfg(test)]
#[path = "yaml_share_workflow_tests.rs"]
mod workflow_tests;
