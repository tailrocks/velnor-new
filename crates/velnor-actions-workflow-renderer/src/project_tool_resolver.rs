//! Bind project version references to the run-time resolver (hard decision 5).
//!
//! Generated text carries `${VELNOR_TOOL_<NAME>}` wherever a version comes
//! from the project's Mise config. This document pass runs on every
//! rendered workflow before node sharing:
//!
//! - each job that references a project version gains the fixed resolver
//!   step after both Mise setup and source checkout; no earlier step may read one;
//! - `run:` text keeps the shell form; every other value (`with:`, `env:`,
//!   cache keys) takes the `${{ env.NAME }}` expression form;
//! - a job-level `RUSTUP_TOOLCHAIN` reference moves into the resolver,
//!   which exports the resolved channel (job env cannot read `env`).

use std::collections::BTreeSet;

use velnor_actions_contract::config::project_tool_refs::{
    PROJECT_TOOLS_STEP_NAME, project_tool_ref_names, project_tool_refs_to_expressions,
};
use velnor_actions_contract::config::project_tool_resolver::project_tool_resolver_script;

use crate::{RenderError, setup::MISE_ACTION_NAME, yaml::Yaml};

#[path = "project_tool_resolver_quote.rs"]
mod quote;
use quote::quote_run_refs;

/// Apply the resolver contract to one rendered workflow document.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] when a job reads a project
/// version without preceding Mise and checkout steps, an earlier step reads
/// one, or workflow/job env carries one outside `RUSTUP_TOOLCHAIN`.
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
            "env" => export_rustup = take_job_rustup(id, value)?,
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
        // Without a checkout the project config is unreadable. Such a job
        // installs no Rust, so a hoisted `RUSTUP_TOOLCHAIN` is inert there and
        // is dropped; any other reference fails closed.
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
    export_rustup |= hoist_early_rustup(id, &mut steps[..=prerequisites])?;
    let already_bound = resolver_at(id, steps, prerequisites + 1)?;
    for step in steps.iter_mut() {
        rewrite_mappings(step, false)?;
    }
    if already_bound {
        return Ok(());
    }
    steps.insert(
        prerequisites + 1,
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(PROJECT_TOOLS_STEP_NAME)),
            ("run".to_owned(), resolver_run(&referenced, export_rustup)),
        ]),
    );
    Ok(())
}

fn hoist_early_rustup(id: &str, steps: &mut [Yaml]) -> Result<bool, RenderError> {
    // Before both prerequisites, only the inert Rustup value may be hoisted.
    // The resolver
    // exports `RUSTUP_TOOLCHAIN` for every later step instead.
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

fn resolver_at(id: &str, steps: &[Yaml], insertion: usize) -> Result<bool, RenderError> {
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
    Ok(true)
}

/// Bind one composite action's steps: insert the resolver after its own Mise
/// setup. This runs once per rendered document ([`bind_workflow`]); a
/// composite that already carries the resolver step is left as it is.
///
/// A composite is a separate unit that a caller may use from any job, and a
/// caller job binds only the references written in its own steps, so a
/// composite that reads a project version must resolve it itself.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for unbalanced `run` quoting, or
/// when the composite reads a project version without its own Mise setup.
fn bind_composite_steps(steps: &mut Vec<Yaml>) -> Result<(), RenderError> {
    let referenced: BTreeSet<String> = steps.iter().flat_map(names).collect();
    if referenced.is_empty() {
        return Ok(());
    }
    for step in steps.iter_mut() {
        rewrite_mappings(step, false)?;
    }
    let Some(setup) = steps.iter().position(is_mise_setup) else {
        return Err(invalid("composite_no_mise_setup"));
    };
    if steps.iter().any(is_resolver_step) {
        return Ok(());
    }
    steps.insert(
        setup + 1,
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(PROJECT_TOOLS_STEP_NAME)),
            ("shell".to_owned(), Yaml::str("bash")),
            ("run".to_owned(), resolver_run(&referenced, false)),
        ]),
    );
    Ok(())
}

/// Job and composite resolvers share the same narrowly validated write.
fn resolver_run(referenced: &BTreeSet<String>, export_rustup: bool) -> Yaml {
    // zizmor audits every composite and privileged job `GITHUB_ENV` write;
    // this one appends only digits-and-dots values the script validated.
    Yaml::annotated(
        project_tool_resolver_script(referenced, export_rustup),
        "zizmor: ignore[github-env]",
    )
}

/// Whether `step` is the generated resolver step.
fn is_resolver_step(step: &Yaml) -> bool {
    matches!(step, Yaml::Map(fields) if fields.iter().any(|(key, value)| {
        key == "name" && scalar(value) == Some(PROJECT_TOOLS_STEP_NAME)
    }))
}

/// Remove a job-level `RUSTUP_TOOLCHAIN` reference; any other reference fails.
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

/// Remove a step's `env` mapping once it is empty (actionlint rejects it).
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

/// Rewrite every non-`run` scalar to the expression form and quote every
/// `run` reference as one shell word ([`quote_run_refs`]).
fn rewrite_mappings(node: &mut Yaml, in_run: bool) -> Result<(), RenderError> {
    let rewrite = |text: &mut String| -> Result<(), RenderError> {
        *text = if in_run {
            quote_run_refs(text)?
        } else {
            project_tool_refs_to_expressions(text)
        };
        Ok(())
    };
    match node {
        Yaml::Str(text) | Yaml::Quoted(text) => rewrite(text)?,
        Yaml::Annotated { value, .. } | Yaml::AnchoredScalar { value, .. } => rewrite(value)?,
        Yaml::Flow(items) => {
            for item in items {
                rewrite(item)?;
            }
        }
        Yaml::FlowMap(pairs) => {
            for (_, value) in pairs {
                rewrite(value)?;
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                rewrite_mappings(item, in_run)?;
            }
        }
        Yaml::Map(pairs) | Yaml::AnchoredMap { entries: pairs, .. } => {
            for (key, value) in pairs {
                rewrite_mappings(value, key == "run")?;
            }
        }
        Yaml::Null | Yaml::Bool(_) | Yaml::Int(_) | Yaml::Alias(_) => {}
    }
    Ok(())
}

/// Project reference names anywhere under `node`.
fn names(node: &Yaml) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect(node, &mut out);
    out
}

fn add(out: &mut BTreeSet<String>, text: &str) {
    out.extend(project_tool_ref_names(text).into_iter().map(str::to_owned));
}

fn collect(node: &Yaml, out: &mut BTreeSet<String>) {
    match node {
        Yaml::Str(text) | Yaml::Quoted(text) => add(out, text),
        Yaml::Annotated { value, .. } | Yaml::AnchoredScalar { value, .. } => add(out, value),
        Yaml::Flow(items) => {
            for item in items {
                add(out, item);
            }
        }
        Yaml::FlowMap(pairs) => {
            for (_, value) in pairs {
                add(out, value);
            }
        }
        Yaml::Seq(items) => {
            for item in items {
                collect(item, out);
            }
        }
        Yaml::Map(pairs) | Yaml::AnchoredMap { entries: pairs, .. } => {
            for (_, value) in pairs {
                collect(value, out);
            }
        }
        Yaml::Null | Yaml::Bool(_) | Yaml::Int(_) | Yaml::Alias(_) => {}
    }
}

fn scalar(node: &Yaml) -> Option<&str> {
    match node {
        Yaml::Str(text) | Yaml::Quoted(text) => Some(text),
        Yaml::Annotated { value, .. } | Yaml::AnchoredScalar { value, .. } => Some(value),
        _ => None,
    }
}

pub(super) fn invalid(context: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("project_tool_ref_unbound:{context}"))
}

#[cfg(test)]
#[path = "project_tool_resolver_tests.rs"]
mod tests;
