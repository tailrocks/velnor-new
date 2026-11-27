// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::adapter::AdapterRegistry;
use super::context::JobContext;
use super::shell;
use super::token;
use crate::job_message::{ActionReferenceType, ActionStep, ActionStepKind, AgentJobRequestMessage};

pub(super) fn reference(step: &ActionStep) -> Option<String> {
    let reference = step.reference.as_ref()?;
    match reference.kind {
        ActionReferenceType::Script => None,
        ActionReferenceType::ContainerRegistry => Some(format!(
            "docker://{}",
            reference.image.as_deref().unwrap_or_default()
        )),
        ActionReferenceType::Repository => {
            let path: Vec<&str> = [reference.name.as_deref(), reference.path.as_deref()]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect();
            let mut text = path.join("/");
            if let Some(git_ref) = reference.git_ref.as_deref().filter(|part| !part.is_empty()) {
                text.push('@');
                text.push_str(git_ref);
            }
            Some(text)
        }
    }
}

fn present(value: Option<&serde_json::Value>) -> bool {
    value.is_some_and(|value| !value.is_null())
}

fn shell_problem(step: &ActionStep, context: &JobContext) -> Option<String> {
    let spec = shell::admission_spec(step, context)?;
    (!shell::is_supported(&spec)).then(|| format!("shell {spec}"))
}

fn step_problem(
    step: &ActionStep,
    adapters: &AdapterRegistry,
    context: &JobContext,
) -> Option<String> {
    if step.background || step.kind == ActionStepKind::BackgroundStepControl {
        return Some("background step".to_owned());
    }
    if step.reference.is_none() {
        return Some("step without a definition reference".to_owned());
    }
    if let Some(reference) = reference(step) {
        let Some(adapter) = adapters.find(&reference) else {
            return Some(format!("uses {reference}"));
        };
        let inputs = match step.inputs.as_ref() {
            Some(inputs) => match token::declared(inputs, context.base_env()) {
                Some(declared) => declared,
                None => return Some(format!("{reference} input name is not a literal")),
            },
            None => BTreeMap::new(),
        };
        return adapter.problem(&inputs);
    }
    shell_problem(step, context)
}

pub(super) fn problems(
    message: &AgentJobRequestMessage,
    adapters: &AdapterRegistry,
    context: &JobContext,
) -> Vec<String> {
    let mut found = Vec::new();
    if present(message.job_container.as_ref()) {
        found.push("job container".to_owned());
    }
    let sidecars = message
        .job_sidecar_containers
        .as_ref()
        .is_some_and(|sidecars| !sidecars.is_empty());
    if present(message.job_service_containers.as_ref()) || sidecars {
        found.push("job services".to_owned());
    }
    for step in message.steps.iter().filter(|step| step.enabled) {
        if let Some(problem) = step_problem(step, adapters, context)
            && !found.contains(&problem)
        {
            found.push(problem);
        }
    }
    found
}
