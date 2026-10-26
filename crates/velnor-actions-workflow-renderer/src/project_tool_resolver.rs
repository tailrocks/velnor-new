// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_actions_contract::config::project_tool_refs::PROJECT_TOOLS_STEP_NAME;
use velnor_actions_contract::config::project_tool_resolver::project_tool_resolver_script;

use crate::{RenderError, setup::MISE_ACTION_NAME, yaml::Yaml};

#[path = "project_tool_resolver_quote.rs"]
mod quote;
use quote::quote_run_refs;

#[path = "project_tool_resolver_tree.rs"]
mod tree;
use tree::{names, rewrite_mappings, scalar};
#[path = "project_tool_resolver_verifier.rs"]
mod verifier;

pub fn quote_project_tool_run_refs(text: &str) -> Result<String, RenderError> {
    quote_run_refs(text)
}

pub(crate) fn bind_workflow(document: &mut Yaml) -> Result<(), RenderError> {
    let Yaml::Map(entries) = document else {
        return Ok(());
    };
    for (key, value) in entries.iter_mut() {
        if key == "jobs" {
            let Yaml::Map(jobs) = value else {
                continue;
            };
            for (id, job) in jobs.iter_mut() {
                bind_job(id, job)?;
            }
        } else if key == "runs" {
            if let Yaml::Map(runs) = value
                && let Some((_, Yaml::Seq(steps))) = runs.iter_mut().find(|(key, _)| key == "steps")
            {
                bind_composite_steps(steps)?;
            }
        } else if !names(value).is_empty() {
            return Err(invalid(&format!("workflow_{key}")));
        }
    }
    Ok(())
}

fn bind_job(id: &str, job: &mut Yaml) -> Result<(), RenderError> {
    let referenced = names(job);
    if referenced.is_empty() {
        return Ok(());
    }
    let Yaml::Map(fields) = job else {
        return Err(invalid(id));
    };
    let mut export_rustup = false;
    for (key, value) in fields.iter_mut() {
        match key.as_str() {
            "steps" => {}
            "env" => {
                export_rustup = take_job_rustup(id, value)?;
            }
            _ if !names(value).is_empty() => return Err(invalid(&format!("{id}:{key}"))),
            _ => {}
        }
    }
    fields.retain(|(key, value)| {
        key != "env" || !matches!(value, Yaml::Map(pairs) if pairs.is_empty())
    });
    let Some((_, Yaml::Seq(steps))) = fields.iter_mut().find(|(key, _)| key == "steps") else {
        return Err(invalid(id));
    };
    let Some(checkout) = steps.iter().position(is_checkout) else {
        for step in steps.iter_mut() {
            if let Yaml::Map(step_fields) = step
                && let Some((_, env)) = step_fields.iter_mut().find(|(key, _)| key == "env")
            {
                take_job_rustup(id, env)?;
            }
            drop_empty_env(step);
        }
        return if steps.iter().all(|step| names(step).is_empty()) {
            Ok(())
        } else {
            Err(invalid(&format!("{id}:no_checkout")))
        };
    };
    let Some(setup) = steps.iter().position(is_mise_setup) else {
        return Err(invalid(&format!("{id}:no_mise_setup")));
    };
    let prerequisites = setup.max(checkout);
    let verifier_positions = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| verifier::verifier_action_ref(step).map(|_| index))
        .collect::<Vec<_>>();
    let insertion = prerequisites + 1;
    if verifier_positions
        .iter()
        .any(|position| *position < insertion)
    {
        return Err(invalid(&format!("{id}:verifier_before_project_resolver")));
    }
    export_rustup |= hoist_early_rustup(id, &mut steps[..=prerequisites])?;
    let resolver = resolver_step(&referenced, export_rustup, false)?;
    let already_bound = resolver_at(id, steps, insertion, &resolver)?;
    if already_bound
        && let Some(resolver) = steps.iter().position(is_resolver_step)
        && verifier_positions
            .iter()
            .any(|position| *position < resolver)
    {
        return Err(invalid(&format!("{id}:verifier_before_project_resolver")));
    }
    for step in steps.iter_mut() {
        rewrite_mappings(step, false)?;
    }
    if already_bound {
        return Ok(());
    }
    steps.insert(insertion, resolver);
    Ok(())
}

fn hoist_early_rustup(id: &str, steps: &mut [Yaml]) -> Result<bool, RenderError> {
    let mut export_rustup = false;
    for step in steps.iter_mut() {
        if let Yaml::Map(fields) = step
            && let Some((_, env)) = fields.iter_mut().find(|(key, _)| key == "env")
        {
            export_rustup |= take_job_rustup(id, env).unwrap_or(false);
        }
        drop_empty_env(step);
    }
    if let Some(early) = steps.iter().find(|step| !names(step).is_empty()) {
        let name = match early {
            Yaml::Map(fields) => fields
                .iter()
                .find(|(key, _)| key == "name")
                .and_then(|(_, value)| scalar(value))
                .unwrap_or("unnamed"),
            _ => "unnamed",
        };
        return Err(invalid(&format!("{id}:read_before_setup:{name}")));
    }
    Ok(export_rustup)
}

fn resolver_at(
    id: &str,
    steps: &[Yaml],
    insertion: usize,
    expected: &Yaml,
) -> Result<bool, RenderError> {
    let positions = steps
        .iter()
        .enumerate()
        .filter(|(_, step)| is_resolver_step(step))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if positions.is_empty() {
        return Ok(false);
    }
    if positions != [insertion] {
        return Err(invalid(&format!("{id}:resolver_prerequisite_order")));
    }
    if steps.get(insertion) != Some(expected) {
        return Err(invalid(&format!("{id}:resolver_step_tampered")));
    }
    Ok(true)
}

fn bind_composite_steps(steps: &mut Vec<Yaml>) -> Result<(), RenderError> {
    let referenced: BTreeSet<String> = steps.iter().flat_map(names).collect();
    if referenced.is_empty() {
        return Ok(());
    }
    for step in steps.iter_mut() {
        rewrite_mappings(step, false)?;
    }
    if steps.iter().any(is_resolver_step) {
        return Ok(());
    }
    let at = steps
        .iter()
        .position(is_mise_setup)
        .map_or(0, |setup| setup + 1);
    steps.insert(at, resolver_step(&referenced, false, true)?);
    Ok(())
}

fn resolver_step(
    referenced: &BTreeSet<String>,
    export_rustup: bool,
    composite: bool,
) -> Result<Yaml, RenderError> {
    crate::verifier_shell::render_project_tool_resolver(
        &project_tool_resolver_script(referenced, export_rustup),
        composite,
    )
}

fn is_resolver_step(step: &Yaml) -> bool {
    matches!(step, Yaml::Map(fields) if fields.iter().any(|(key, value)| {
        key == "name" && scalar(value) == Some(PROJECT_TOOLS_STEP_NAME)
    }))
}

fn take_job_rustup(id: &str, env: &mut Yaml) -> Result<bool, RenderError> {
    let Yaml::Map(pairs) = env else {
        return Ok(false);
    };
    let before = pairs.len();
    pairs.retain(|(key, value)| key != "RUSTUP_TOOLCHAIN" || names(value).is_empty());
    if pairs.iter().any(|(_, value)| !names(value).is_empty()) {
        return Err(invalid(&format!("{id}:job_env")));
    }
    Ok(pairs.len() != before)
}

fn drop_empty_env(step: &mut Yaml) {
    if let Yaml::Map(fields) = step {
        fields.retain(|(key, value)| {
            key != "env" || !matches!(value, Yaml::Map(pairs) if pairs.is_empty())
        });
    }
}

fn is_checkout(step: &Yaml) -> bool {
    uses_starts_with(step, "actions/checkout@")
}

fn is_mise_setup(step: &Yaml) -> bool {
    uses_starts_with(step, &format!("{MISE_ACTION_NAME}@"))
}

fn uses_starts_with(step: &Yaml, prefix: &str) -> bool {
    let Yaml::Map(fields) = step else {
        return false;
    };
    fields.iter().any(|(key, value)| {
        key == "uses" && scalar(value).is_some_and(|uses| uses.starts_with(prefix))
    })
}

pub(super) fn invalid(context: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("project_tool_ref_unbound:{context}"))
}
