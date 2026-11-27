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
const INPUTS: [&str; 4] = [
    "version",
    "sha256-linux-x64",
    "sha256-linux-arm64",
    "sha256-macos-arm64",
];
const ACTION_PATH_NAME: &str = "action_path";
const ACTION_PATH_PREFIX: &str = "github_";

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
        if definition
            .default
            .as_deref()
            .is_some_and(|default| default.contains("${{"))
        {
            return Err(ExecError::Unsupported(format!(
                "setup-velnor input '{name}' has a default that is an expression"
            )));
        }
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

fn render(text: &str, env: &ExprEnv) -> Result<String, ExecError> {
    evaluate_template(text, env).map_err(refuse)
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
        let reads_action_path = [raw.name.as_deref(), Some(shell), Some(script)]
            .into_iter()
            .flatten()
            .chain(raw.env.values().map(String::as_str))
            .any(names_action_path);
        if reads_action_path {
            return Err(ExecError::Unsupported(
                "setup-velnor step that reads github.action_path or GITHUB_ACTION_PATH".to_owned(),
            ));
        }
        let step_env: Vec<(String, String)> = raw
            .env
            .iter()
            .map(|(name, value)| Ok((name.clone(), render(value, &env)?)))
            .collect::<Result<_, ExecError>>()?;
        let mut scoped = env.clone();
        scoped.merge_strings(RootContext::Env, &step_env);
        steps.push(ExpandedStep {
            name: raw
                .name
                .as_deref()
                .map_or_else(|| Ok(String::new()), |name| render(name, &scoped))?,
            shell: render(shell, &scoped)?,
            env: step_env,
            script: render(script, &scoped)?,
        });
    }
    if steps.len() > 1 {
        return Err(ExecError::Unsupported(
            "setup-velnor with more than one step: GITHUB_ENV and GITHUB_PATH changes do not reach later steps of the same action".to_owned(),
        ));
    }
    Ok(steps)
}

fn names_action_path(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    lower.match_indices(ACTION_PATH_NAME).any(|(start, found)| {
        let end = start + found.len();
        let head = &lower[..start];
        let head = head.strip_suffix(ACTION_PATH_PREFIX).unwrap_or(head);
        !head.bytes().next_back().is_some_and(is_word)
            && !lower.as_bytes().get(end).copied().is_some_and(is_word)
    })
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

fn require_inputs(inputs: &BTreeMap<String, InputValue>) -> Result<(), ExecError> {
    if let Some(name) = inputs
        .keys()
        .find(|name| !INPUTS.contains(&name.to_ascii_lowercase().as_str()))
    {
        return Err(ExecError::Unsupported(format!(
            "setup-velnor input '{name}' is not supported by velnor-runner"
        )));
    }
    if !inputs
        .keys()
        .any(|name| name.eq_ignore_ascii_case("version"))
    {
        return Err(ExecError::Invalid(
            "setup-velnor input 'version' is required".to_owned(),
        ));
    }
    Ok(())
}

impl SetupVelnor {
    fn accepted<'a>(
        &'a self,
        reference: &'a str,
        inputs: &BTreeMap<String, InputValue>,
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
        require_inputs(inputs)?;
        Ok((facts, sha))
    }
}

impl StepAdapter for SetupVelnor {
    fn handles(&self, reference: &str) -> bool {
        reference.starts_with(PREFIX)
    }

    fn problem(&self, reference: &str, inputs: &BTreeMap<String, InputValue>) -> Option<String> {
        self.accepted(reference, inputs).err().map(super::reason)
    }

    fn run<'a>(&'a self, ctx: &'a mut StepContext<'_>) -> StepFuture<'a> {
        async move {
            let reference = ctx.reference.clone();
            let given: BTreeMap<String, String> = ctx
                .inputs
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            let admitted = given
                .iter()
                .map(|(name, value)| (name.clone(), InputValue::Literal(value.clone())))
                .collect();
            let (facts, sha) = self.accepted(&reference, &admitted)?;
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
