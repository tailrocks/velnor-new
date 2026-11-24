// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};
use crate::expression::is_identifier;
use crate::plan::{Input, InputKind, Output};
use crate::raw::{RawInput, RawOutput};

const MAX_DISPATCH_INPUTS: usize = 25;

pub(super) fn inputs(
    file: &str,
    raw: Vec<RawInput>,
    dispatch: bool,
) -> Result<Vec<Input>, WorkflowsError> {
    if dispatch && raw.len() > MAX_DISPATCH_INPUTS {
        return refuse(format!(
            "the workflow_dispatch trigger of {file} declares {} inputs, and GitHub allows {MAX_DISPATCH_INPUTS}",
            raw.len()
        ));
    }
    let mut inputs: Vec<Input> = Vec::new();
    for input in raw {
        let repeated = inputs
            .iter()
            .any(|seen| seen.name.eq_ignore_ascii_case(&input.name));
        if !is_identifier(&input.name, true) || repeated {
            return refuse(format!(
                "the input {:?} of {file} is not a unique identifier",
                input.name
            ));
        }
        inputs.push(one_input(file, input, dispatch)?);
    }
    Ok(inputs)
}

fn one_input(file: &str, input: RawInput, dispatch: bool) -> Result<Input, WorkflowsError> {
    let name = input.name;
    let kind = match (input.kind.as_deref().unwrap_or("string"), dispatch) {
        ("string", _) => InputKind::String,
        ("boolean", _) => InputKind::Boolean,
        ("number", _) => InputKind::Number,
        ("choice", true) if !input.options.is_empty() => {
            if input
                .options
                .iter()
                .enumerate()
                .any(|(index, option)| option.is_empty() || input.options[..index].contains(option))
            {
                return refuse(format!(
                    "the choice input {name} of {file} needs unique, non-empty options"
                ));
            }
            InputKind::Choice(input.options.clone())
        }
        ("environment", true) => InputKind::Environment,
        (other, _) => {
            return refuse(format!(
                "the input {name} of {file} has type {other:?}, which is not allowed here (a call input is a string, boolean or number; a dispatch input may also be a choice with options, or an environment)"
            ));
        }
    };
    if !matches!(kind, InputKind::Choice(_)) && !input.options.is_empty() {
        return refuse(format!(
            "the input {name} of {file} sets options, which only a choice input takes"
        ));
    }
    let default = input
        .default
        .map(|value| default_text(file, &name, &kind, &value))
        .transpose()?;
    Ok(Input {
        required: input.required.unwrap_or(default.is_none()),
        description: input.description,
        name,
        kind,
        default,
    })
}

fn default_text(
    file: &str,
    name: &str,
    kind: &InputKind,
    value: &toml::Value,
) -> Result<String, WorkflowsError> {
    let text = match (kind, value) {
        (InputKind::Boolean, toml::Value::Boolean(flag)) => Some(flag.to_string()),
        (InputKind::Number, toml::Value::Integer(number)) if *number >= 0 => {
            Some(number.to_string())
        }
        (InputKind::String | InputKind::Environment, toml::Value::String(text)) => {
            Some(text.clone())
        }
        (InputKind::Choice(options), toml::Value::String(text)) if options.contains(text) => {
            Some(text.clone())
        }
        _ => None,
    };
    text.ok_or_else(|| {
        WorkflowsError(format!(
            "the default of input {name} of {file} is not a {} value (a choice default must be one of its options)",
            kind.name()
        ))
    })
}

pub(super) fn call_output(file: &str, raw: RawOutput) -> Result<Output, WorkflowsError> {
    let Some((job, output)) = raw.value.split_once('.') else {
        return refuse(format!(
            "the output {} of {file} must have value <job>.<output>, got {:?}",
            raw.name, raw.value
        ));
    };
    if !is_identifier(&raw.name, true) || !is_identifier(job, true) || !is_identifier(output, true)
    {
        return refuse(format!(
            "the output {} of {file} must be an identifier with value <job>.<output>",
            raw.name
        ));
    }
    Ok(Output {
        description: raw.description,
        from_job: job.to_owned(),
        from_output: output.to_owned(),
        name: raw.name,
    })
}
