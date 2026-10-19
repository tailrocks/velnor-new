//! Bind project version references to the run-time resolver (hard decision 4).
//!
//! Generated text carries `${VELNOR_TOOL_<NAME>}` wherever a version comes
//! from the project's Mise config. This document pass runs on every
//! rendered workflow before node sharing:
//!
//! - each job that references a project version gains the fixed resolver
//!   step directly after its Mise setup, and no earlier step may read one;
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

/// Apply the resolver contract to one rendered workflow document.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] when a job reads a project
/// version without a preceding Mise setup, a step before the setup reads
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
    let Some(setup) = steps.iter().position(is_mise_setup) else {
        return Err(invalid(&format!("{id}:no_mise_setup")));
    };
    // A pre-setup step cannot read the resolved channel; the resolver
    // exports `RUSTUP_TOOLCHAIN` for every later step instead.
    for step in &mut steps[..=setup] {
        if let Yaml::Map(fields) = step
            && let Some((_, env)) = fields.iter_mut().find(|(key, _)| key == "env")
        {
            export_rustup |= take_job_rustup(id, env).unwrap_or(false);
            fields.retain(|(key, value)| {
                key != "env" || !matches!(value, Yaml::Map(pairs) if pairs.is_empty())
            });
        }
    }
    if let Some(early) = steps[..=setup].iter().find(|step| !names(step).is_empty()) {
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
    for step in steps.iter_mut() {
        rewrite_mappings(step, false)?;
    }
    let script = project_tool_resolver_script(&referenced, export_rustup);
    steps.insert(
        setup + 1,
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(PROJECT_TOOLS_STEP_NAME)),
            ("run".to_owned(), Yaml::str(script)),
        ]),
    );
    Ok(())
}

/// Bind one composite action's steps: insert the resolver after its own Mise
/// setup when one exists; otherwise the calling job's resolver already ran.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for unbalanced `run` quoting.
pub(crate) fn bind_composite_steps(steps: &mut Vec<Yaml>) -> Result<(), RenderError> {
    let referenced: BTreeSet<String> = steps.iter().flat_map(names).collect();
    if referenced.is_empty() {
        return Ok(());
    }
    for step in steps.iter_mut() {
        rewrite_mappings(step, false)?;
    }
    if let Some(setup) = steps.iter().position(is_mise_setup) {
        steps.insert(
            setup + 1,
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(PROJECT_TOOLS_STEP_NAME)),
                ("shell".to_owned(), Yaml::str("bash")),
                // zizmor audits every composite `GITHUB_ENV` write; this one
                // appends only digits-and-dots values the script validated.
                (
                    "run".to_owned(),
                    Yaml::annotated(
                        project_tool_resolver_script(&referenced, false),
                        "zizmor: ignore[github-env]",
                    ),
                ),
            ]),
        );
    }
    Ok(())
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

fn is_mise_setup(step: &Yaml) -> bool {
    let Yaml::Map(fields) = step else {
        return false;
    };
    fields.iter().any(|(key, value)| {
        key == "uses"
            && scalar(value).is_some_and(|uses| uses.starts_with(&format!("{MISE_ACTION_NAME}@")))
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

/// Quote each `${VELNOR_TOOL_<NAME>}` in shell text as its own expansion:
/// unquoted becomes `"${..}"`, single-quoted becomes `'"${..}"'` (the
/// outer shell expands it before any nested `sh -c` sees the literal
/// version), double-quoted stays. Unbalanced quoting fails closed.
fn quote_run_refs(text: &str) -> Result<String, RenderError> {
    use velnor_actions_contract::config::project_tool_refs::PROJECT_TOOL_REF_PREFIX;
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Frame {
        /// Unquoted text (top level or inside `$( )`).
        Plain,
        /// Inside `"..."`.
        Double,
        /// Inside `'...'`.
        Single,
    }
    if project_tool_ref_names(text).is_empty() {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(text.len() + 8);
    // `$(` opens a fresh plain frame even inside double quotes.
    let mut stack = vec![Frame::Plain];
    let mut escaped = false;
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        let frame = *stack.last().unwrap_or(&Frame::Plain);
        if !escaped && rest.starts_with(PROJECT_TOOL_REF_PREFIX) {
            let end = rest.find('}').ok_or_else(|| invalid("run_ref_unclosed"))? + 1;
            let reference = &rest[..end];
            match frame {
                Frame::Double => out.push_str(reference),
                Frame::Plain => {
                    for part in ["\"", reference, "\""] {
                        out.push_str(part);
                    }
                }
                Frame::Single => {
                    for part in ["'\"", reference, "\"'"] {
                        out.push_str(part);
                    }
                }
            }
            rest = &rest[end..];
            continue;
        }
        let mut width = ch.len_utf8();
        match (frame, ch) {
            _ if escaped => escaped = false,
            (Frame::Plain | Frame::Double, '\\') => escaped = true,
            (Frame::Plain | Frame::Double, '$') if rest.starts_with("$(") => {
                stack.push(Frame::Plain);
                width = 2;
            }
            (Frame::Plain, ')') if stack.len() > 1 => {
                stack.pop();
            }
            (Frame::Plain, '\'') => stack.push(Frame::Single),
            (Frame::Single, '\'') | (Frame::Double, '"') => {
                stack.pop();
            }
            (Frame::Plain, '"') => stack.push(Frame::Double),
            _ => {}
        }
        out.push_str(&rest[..width]);
        rest = &rest[width..];
    }
    if stack.len() != 1 || escaped {
        return Err(invalid("run_quoting_unbalanced"));
    }
    Ok(out)
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

fn invalid(context: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("project_tool_ref_unbound:{context}"))
}

#[cfg(test)]
#[path = "project_tool_resolver_tests.rs"]
mod tests;
