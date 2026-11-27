// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use futures_util::FutureExt;
use serde::Deserialize;

use super::git::read_blob;
use crate::broker::{TrustedFacts, mirror_path};
use crate::context_value::ContextValue;
use crate::exec::{ExecError, InputValue, StepAdapter, StepContext, StepFuture, StepResult};
use crate::expr::{ExprEnv, ExprError, ParseErrorKind, RootContext, evaluate_template};
use crate::store::store_root;

const PREFIX: &str = "tailrocks/velnor-new/actions/setup-velnor@";
const ACTION_PATH: &str = "actions/setup-velnor/action.yml";
const OWNER_REPOSITORY: &str = "tailrocks/velnor-new";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    #[serde(rename = "name")]
    _name: Option<String>,
    #[serde(rename = "description")]
    _description: Option<String>,
    #[serde(default)]
    inputs: BTreeMap<String, InputDefinition>,
    runs: Runs,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputDefinition {
    #[serde(rename = "description")]
    _description: Option<String>,
    default: Option<String>,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    let declared: BTreeSet<String> = action
        .inputs
        .keys()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    if let Some(name) = given
        .keys()
        .find(|name| !declared.contains(&name.to_ascii_lowercase()))
    {
        return Err(ExecError::Unsupported(format!(
            "setup-velnor input '{name}' is not supported by velnor-runner"
        )));
    }
    let given: BTreeMap<String, &String> = given
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect();
    let mut entries = Vec::new();
    for (name, definition) in &action.inputs {
        let value = given
            .get(&name.to_ascii_lowercase())
            .copied()
            .or(definition.default.as_ref());
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

fn refuse(error: ExprError) -> ExecError {
    match error {
        ExprError::Parse(parse)
            if matches!(
                parse.kind,
                ParseErrorKind::UnsupportedContext | ParseErrorKind::UnsupportedFunction
            ) =>
        {
            ExecError::Unsupported(format!("setup-velnor expression: {parse}"))
        }
        other => ExecError::from(other),
    }
}

pub(crate) fn expand(
    action_yml: &str,
    inputs: &BTreeMap<String, String>,
    outer: &ExprEnv,
) -> Result<Vec<ExpandedStep>, ExecError> {
    let action: Action = serde_saphyr::from_str(action_yml)
        .map_err(|error| ExecError::Invalid(format!("setup-velnor action.yml: {error}")))?;
    if action.runs.using != "composite" {
        return Err(ExecError::Unsupported(
            "setup-velnor is not a composite action".to_owned(),
        ));
    }
    if action.runs.steps.is_empty() {
        return Err(ExecError::Invalid(
            "setup-velnor action.yml declares no steps".to_owned(),
        ));
    }
    let mut env = outer.clone();
    env.set_context(RootContext::Inputs, &resolve_inputs(&action, inputs)?);
    env.set_context(
        RootContext::Steps,
        &ContextValue::object(Vec::new()).map_err(|error| ExecError::Invalid(error.to_string()))?,
    );
    env.withhold(RootContext::Secrets);
    env.withhold(RootContext::Needs);
    let render = |text: &str| evaluate_template(text, &env).map_err(refuse);
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
            shell: render(shell)?,
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

fn require_commit_sha(sha: &str) -> Result<(), ExecError> {
    if sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }
    Err(ExecError::Invalid(format!(
        "setup-velnor reference '{sha}' is not a 40 digit commit sha"
    )))
}

async fn pinned_action_yml(
    ctx: &mut StepContext<'_>,
    facts: &TrustedFacts,
    sha: &str,
) -> Result<String, ExecError> {
    let mirror = mirror_path(&store_root(), facts)
        .map_err(|error| ExecError::Invalid(format!("{error:#}")))?;
    read_blob(ctx, &mirror, sha, ACTION_PATH).await
}

#[derive(Debug)]
pub(super) struct SetupVelnor {
    pub(super) facts: Result<TrustedFacts, String>,
}

impl SetupVelnor {
    fn accepted<'a>(
        &'a self,
        reference: &'a str,
    ) -> Result<(&'a TrustedFacts, &'a str), ExecError> {
        let facts = self
            .facts
            .as_ref()
            .map_err(|reason| ExecError::Invalid(reason.clone()))?;
        if !facts
            .repository
            .as_str()
            .eq_ignore_ascii_case(OWNER_REPOSITORY)
        {
            return Err(ExecError::Unsupported(format!(
                "setup-velnor is only supported in {OWNER_REPOSITORY}, not {}",
                facts.repository.as_str()
            )));
        }
        let sha = reference.strip_prefix(PREFIX).unwrap_or(reference);
        require_commit_sha(sha)?;
        Ok((facts, sha))
    }
}

impl StepAdapter for SetupVelnor {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with(PREFIX)
    }

    fn problem(&self, reference: &str, _inputs: &BTreeMap<String, InputValue>) -> Option<String> {
        self.accepted(reference).err().map(super::reason)
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let reference = ctx.reference.clone();
            let (facts, sha) = self.accepted(&reference)?;
            let given = ctx
                .inputs
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            let yml = pinned_action_yml(ctx, facts, sha).await?;
            let steps = expand(&yml, &given, &ctx.expressions)?;
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
