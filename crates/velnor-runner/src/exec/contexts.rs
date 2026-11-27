// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::{ExecError, RunOptions, evaluate, private, token};
use crate::context_value::{ContextValue, ordinal_ignore_case_eq};
use crate::expr::ExprEnv;
use crate::job_message::AgentJobRequestMessage;
use crate::protocol::http::runner_architecture;

const GITHUB_EXPORTS: [&str; 37] = [
    "action",
    "action_path",
    "action_ref",
    "action_repository",
    "actor",
    "actor_id",
    "api_url",
    "base_ref",
    "env",
    "event_name",
    "event_path",
    "graphql_url",
    "head_ref",
    "job",
    "output",
    "path",
    "ref_name",
    "ref_protected",
    "ref_type",
    "ref",
    "repository",
    "repository_id",
    "repository_owner",
    "repository_owner_id",
    "retention_days",
    "run_attempt",
    "run_id",
    "run_number",
    "server_url",
    "sha",
    "state",
    "step_summary",
    "triggering_actor",
    "workflow",
    "workflow_ref",
    "workflow_sha",
    "workspace",
];

const SELF_HOSTED: &str = "self-hosted";
pub(super) const EVENT_DIRECTORY: &str = "_github_workflow";
const GITHUB_TOKEN_VARIABLE: &str = "system.github.token";
const ACCESS_TOKEN_VARIABLE: &str = "system.accesstoken";
const STEP_DEBUG_VARIABLE: &str = "ACTIONS_STEP_DEBUG";

pub(super) fn set_pair(pairs: &mut Vec<(String, String)>, name: &str, value: &str) {
    pairs.retain(|(known, _)| known != name);
    pairs.push((name.to_owned(), value.to_owned()));
}

pub(super) fn set_output(outputs: &mut Vec<(String, String)>, name: &str, value: &str) {
    match outputs
        .iter_mut()
        .find(|(known, _)| ordinal_ignore_case_eq(known, name))
    {
        Some((_, known)) => value.clone_into(known),
        None => outputs.push((name.to_owned(), value.to_owned())),
    }
}

pub(super) fn string(value: &str) -> ContextValue {
    ContextValue::String(value.to_owned())
}

pub(super) fn object(pairs: &[(String, String)], case_sensitive: bool) -> ContextValue {
    ContextValue::Object {
        case_sensitive,
        entries: pairs
            .iter()
            .map(|(name, value)| (name.clone(), string(value)))
            .collect(),
    }
}

pub(super) fn variable<'a>(message: &'a AgentJobRequestMessage, name: &str) -> Option<&'a str> {
    message
        .variables
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .and_then(|(_, variable)| variable.value.as_ref().map(|value| value.expose().as_str()))
}

pub(super) fn upsert(entries: &mut Vec<(String, ContextValue)>, name: &str, value: ContextValue) {
    entries.retain(|(known, _)| !known.eq_ignore_ascii_case(name));
    entries.push((name.to_owned(), value));
}

pub(super) fn secrets_context(message: &AgentJobRequestMessage) -> ContextValue {
    let pairs: Vec<(String, String)> = message
        .variables
        .iter()
        .filter(|(name, variable)| {
            variable.is_secret
                && !name.eq_ignore_ascii_case(ACCESS_TOKEN_VARIABLE)
                && !name.eq_ignore_ascii_case(GITHUB_TOKEN_VARIABLE)
        })
        .filter_map(|(name, variable)| {
            Some((name.clone(), variable.value.as_ref()?.expose().clone()))
        })
        .collect();
    object(&pairs, false)
}

pub(super) fn github_context(
    message: &AgentJobRequestMessage,
    source: Option<&ContextValue>,
    options: &RunOptions,
    event_path: &Path,
) -> ContextValue {
    let mut entries = Vec::new();
    if let Some(value) = variable(message, GITHUB_TOKEN_VARIABLE) {
        upsert(&mut entries, "token", string(value));
    }
    if let Some(value) = variable(message, "system.github.job").filter(|job| !job.is_empty()) {
        upsert(&mut entries, "job", string(value));
    }
    for (name, value) in source.and_then(ContextValue::entries).unwrap_or_default() {
        upsert(&mut entries, name, value.clone());
    }
    upsert(
        &mut entries,
        "workspace",
        string(&options.workspace.to_string_lossy()),
    );
    upsert(
        &mut entries,
        "event_path",
        string(&event_path.to_string_lossy()),
    );
    ContextValue::Object {
        case_sensitive: false,
        entries,
    }
}

pub(super) fn with_action(
    github: &ContextValue,
    action: &str,
    repository: Option<(&str, &str)>,
) -> ContextValue {
    let mut entries = github.entries().unwrap_or_default().to_vec();
    upsert(&mut entries, "action", string(action));
    entries.retain(|(name, _)| {
        !name.eq_ignore_ascii_case("action_repository") && !name.eq_ignore_ascii_case("action_ref")
    });
    if let Some((name, git_ref)) = repository {
        upsert(&mut entries, "action_repository", string(name));
        if !git_ref.is_empty() {
            upsert(&mut entries, "action_ref", string(git_ref));
        }
    }
    ContextValue::Object {
        case_sensitive: false,
        entries,
    }
}

pub(super) fn service_variables(message: &AgentJobRequestMessage) -> Vec<(String, String)> {
    let Ok(connection) = message.system_connection() else {
        return Vec::new();
    };
    let url = connection
        .data_string("GenerateIdTokenUrl")
        .filter(|url| !url.is_empty());
    let token = connection
        .authorization
        .as_ref()
        .and_then(|authorization| authorization.parameter_string("AccessToken"));
    match (url, token) {
        (Some(url), Some(token)) => vec![
            ("ACTIONS_ID_TOKEN_REQUEST_URL".to_owned(), url.to_owned()),
            (
                "ACTIONS_ID_TOKEN_REQUEST_TOKEN".to_owned(),
                token.to_owned(),
            ),
        ],
        _ => Vec::new(),
    }
}

pub(super) fn runner_context(
    message: &AgentJobRequestMessage,
    options: &RunOptions,
) -> Result<ContextValue, ExecError> {
    let arch = runner_architecture().map_err(|error| ExecError::Invalid(format!("{error:#}")))?;
    let environment = variable(message, "system.runnerEnvironment").unwrap_or(SELF_HOSTED);
    let mut pairs = vec![
        ("os".to_owned(), options.os.as_str().to_owned()),
        ("arch".to_owned(), arch.to_owned()),
        ("name".to_owned(), options.runner_name.clone()),
        (
            "temp".to_owned(),
            options.temp.to_string_lossy().into_owned(),
        ),
        (
            "tool_cache".to_owned(),
            options.tool_cache.to_string_lossy().into_owned(),
        ),
        ("environment".to_owned(), environment.to_owned()),
    ];
    if variable(message, STEP_DEBUG_VARIABLE).is_some_and(|value| {
        value
            .trim_matches(|character: char| character.is_whitespace() || character == '\0')
            .eq_ignore_ascii_case("true")
    }) {
        pairs.push(("debug".to_owned(), "1".to_owned()));
    }
    Ok(object(&pairs, false))
}

pub(super) fn exported_variables(
    github: &ContextValue,
    runner: &ContextValue,
) -> Vec<(String, String)> {
    let mut variables = vec![("GITHUB_ACTIONS".to_owned(), "true".to_owned())];
    let github_entries = github.entries().unwrap_or_default();
    let allowed = github_entries.iter().filter(|(name, _)| {
        GITHUB_EXPORTS
            .iter()
            .any(|key| key.eq_ignore_ascii_case(name))
    });
    let runner_entries = runner.entries().unwrap_or_default();
    let prefixed = allowed
        .map(|entry| ("GITHUB_", entry))
        .chain(runner_entries.iter().map(|entry| ("RUNNER_", entry)));
    for (prefix, (name, value)) in prefixed {
        let text = match value {
            ContextValue::String(text) => text.clone(),
            ContextValue::Bool(flag) => flag.to_string(),
            _ => continue,
        };
        set_pair(
            &mut variables,
            &format!("{prefix}{}", name.to_ascii_uppercase()),
            &text,
        );
    }
    variables
}

pub(super) async fn write_event(path: &Path, github: &ContextValue) -> Result<(), ExecError> {
    let body = github
        .get("event")
        .map(ContextValue::to_github_json)
        .transpose()
        .map_err(|error| ExecError::Invalid(format!("github.event: {error}")))?
        .unwrap_or_else(|| "{}".to_owned());
    if let Some(parent) = path.parent() {
        private::create_dir(parent).await?;
    }
    private::write_replacing(path, body.as_bytes()).await
}

pub(super) async fn run_defaults(
    message: &AgentJobRequestMessage,
    env: &ExprEnv,
) -> Result<Vec<(String, String)>, ExecError> {
    let mut run = Vec::new();
    for defaults in &message.defaults {
        if let Some(entry) = token::find(defaults, "run") {
            run.clear();
            for (name, value) in evaluate::mapping(entry, env).await? {
                if !value.is_empty() {
                    set_output(&mut run, &name, &value);
                }
            }
        }
    }
    Ok(run)
}
