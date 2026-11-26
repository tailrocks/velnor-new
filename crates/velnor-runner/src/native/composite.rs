// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::PathBuf;

use futures_util::FutureExt;
use serde::Deserialize;

use super::git::read_blob;
use crate::broker::{TrustedFacts, mirror_path};
use crate::context_value::ContextValue;
use crate::exec::{ExecError, StepAdapter, StepContext, StepFuture, StepResult};
use crate::expr::{ExprEnv, RootContext, evaluate_template};
use crate::store::store_root;

const PREFIX: &str = "tailrocks/velnor-new/actions/setup-velnor@";
const ACTION_PATH: &str = "actions/setup-velnor/action.yml";

#[derive(Debug, Deserialize)]
struct Action {
    #[serde(default)]
    inputs: BTreeMap<String, InputDefinition>,
    runs: Runs,
}

#[derive(Debug, Deserialize)]
struct InputDefinition {
    default: Option<String>,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Deserialize)]
struct Runs {
    using: String,
    #[serde(default)]
    steps: Vec<RawStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStep {
    name: Option<String>,
    shell: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    run: Option<String>,
    uses: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ExpandedStep {
    pub(crate) name: String,
    pub(crate) shell: String,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) script: String,
}

fn resolve_inputs(
    action: &Action,
    given: &BTreeMap<String, String>,
) -> Result<ContextValue, ExecError> {
    if let Some(name) = given.keys().find(|name| !action.inputs.contains_key(*name)) {
        return Err(ExecError::Unsupported(format!(
            "setup-velnor input '{name}' is not supported by velnor-runner"
        )));
    }
    let mut entries = Vec::new();
    for (name, definition) in &action.inputs {
        let value = given.get(name).or(definition.default.as_ref());
        match value {
            Some(value) => entries.push((name.clone(), ContextValue::String(value.clone()))),
            None if definition.required => {
                return Err(ExecError::Invalid(format!(
                    "setup-velnor input '{name}' is required"
                )));
            }
            None => entries.push((name.clone(), ContextValue::String(String::new()))),
        }
    }
    ContextValue::object(entries).map_err(|error| ExecError::Invalid(error.to_string()))
}

pub(crate) fn expand(
    action_yml: &str,
    inputs: &BTreeMap<String, String>,
) -> Result<Vec<ExpandedStep>, ExecError> {
    let action: Action = serde_saphyr::from_str(action_yml)
        .map_err(|error| ExecError::Invalid(format!("setup-velnor action.yml: {error}")))?;
    if action.runs.using != "composite" {
        return Err(ExecError::Unsupported(
            "setup-velnor is not a composite action".to_owned(),
        ));
    }
    let mut env = ExprEnv::new(PathBuf::new());
    env.set_context(RootContext::Inputs, &resolve_inputs(&action, inputs)?);
    let render = |text: &str| evaluate_template(text, &env).map_err(ExecError::from);
    let mut steps = Vec::new();
    for raw in &action.runs.steps {
        if raw.uses.is_some() {
            return Err(ExecError::Unsupported(
                "nested uses in setup-velnor".to_owned(),
            ));
        }
        let (Some(script), Some(shell)) = (&raw.run, &raw.shell) else {
            return Err(ExecError::Invalid(
                "a setup-velnor step needs both run and shell".to_owned(),
            ));
        };
        steps.push(ExpandedStep {
            name: raw
                .name
                .as_deref()
                .map_or_else(|| Ok(String::new()), render)?,
            shell: shell.clone(),
            env: raw
                .env
                .iter()
                .map(|(name, value)| Ok((name.clone(), render(value)?)))
                .collect::<Result<_, ExecError>>()?,
            script: render(script)?,
        });
    }
    if steps.len() > 1 {
        return Err(ExecError::Unsupported(
            "setup-velnor with more than one step: GITHUB_ENV and GITHUB_PATH changes do not reach later steps of the same action".to_owned(),
        ));
    }
    Ok(steps)
}

async fn pinned_action_yml(
    ctx: &mut StepContext<'_>,
    facts: &TrustedFacts,
    sha: &str,
) -> Result<String, ExecError> {
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ExecError::Invalid(format!(
            "setup-velnor reference '{sha}' is not a 40 digit commit sha"
        )));
    }
    let mirror = mirror_path(&store_root(), facts)
        .map_err(|error| ExecError::Invalid(format!("{error:#}")))?;
    read_blob(ctx, &mirror, sha, ACTION_PATH).await
}

#[derive(Debug)]
pub(super) struct SetupVelnor {
    pub(super) facts: Result<TrustedFacts, String>,
}

impl StepAdapter for SetupVelnor {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with(PREFIX)
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let sha = ctx.reference.trim_start_matches(PREFIX).to_owned();
            let facts = self
                .facts
                .as_ref()
                .map_err(|reason| ExecError::Invalid(reason.clone()))?;
            let steps = expand(&pinned_action_yml(ctx, facts, &sha).await?, &ctx.inputs)?;
            for step in steps {
                ctx.log(&format!("setup-velnor step: {}", step.name));
                let result = ctx.run_script(&step.script, &step.shell, &step.env).await?;
                if result != StepResult::Succeeded {
                    return Ok(result);
                }
            }
            Ok(StepResult::Succeeded)
        }
        .boxed()
    }
}
