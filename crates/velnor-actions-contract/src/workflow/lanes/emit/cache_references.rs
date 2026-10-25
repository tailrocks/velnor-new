use super::QUALIFICATION_CACHE_OWNER_ID_ENV;
use crate::errors::ContractError;
use crate::workflow::ir::Job;
use crate::workflow::step_identity::{
    QualificationCacheCompositeId, QualificationCacheStepId, StepId,
};
use std::collections::BTreeMap;

pub(super) fn cache_owner_references(
    job: &Job,
    source_id: &str,
    output_id: &str,
) -> Result<BTreeMap<String, String>, ContractError> {
    let mut references = BTreeMap::new();
    for step in &job.steps {
        let is_cache_step = matches!(
            step.id,
            Some(StepId::QualificationCache(_) | StepId::QualificationCacheComposite(_))
        );
        if is_cache_step {
            let owner_id = match &step.kind {
                crate::workflow::step::StepKind::Shell { env, .. }
                | crate::workflow::step::StepKind::Internal { env, .. }
                | crate::workflow::step::StepKind::Action { env, .. }
                | crate::workflow::step::StepKind::TaskExecution { env, .. } => {
                    env.get(QUALIFICATION_CACHE_OWNER_ID_ENV)
                }
                crate::workflow::step::StepKind::NativeCheckExecution { .. } => None,
            };
            if owner_id.is_some_and(|owner_id| owner_id != source_id) {
                return Err(cache_owner_mismatch());
            }
        }
        match step.id {
            Some(StepId::QualificationCache(identity)) => {
                if !identity.belongs_to(source_id) {
                    return Err(cache_owner_mismatch());
                }
                let replacement =
                    QualificationCacheStepId::new(identity.stage(), identity.layer(), output_id)?;
                references.insert(identity.as_string(), replacement.as_string());
            }
            Some(StepId::QualificationCacheComposite(identity)) => {
                if !identity.belongs_to(source_id) {
                    return Err(cache_owner_mismatch());
                }
                let replacement = match identity.layer() {
                    Some(layer) => QualificationCacheCompositeId::for_layer(
                        identity.phase(),
                        layer,
                        output_id,
                    )?,
                    None => QualificationCacheCompositeId::new(identity.phase(), output_id)?,
                };
                references.insert(identity.as_string(), replacement.as_string());
            }
            _ => {}
        }
    }
    Ok(references)
}

fn cache_owner_mismatch() -> ContractError {
    ContractError::identity(
        "workflow.jobs.steps.id",
        "qualification_cache_owner_mismatch",
    )
}

pub(super) fn retarget_owner_env(
    env: &mut BTreeMap<String, String>,
    source_id: &str,
    output_id: &str,
) {
    if let Some(owner_id) = env.get_mut(QUALIFICATION_CACHE_OWNER_ID_ENV)
        && owner_id == source_id
    {
        output_id.clone_into(owner_id);
    }
}

pub(super) fn rewrite_expression_map(
    values: &mut BTreeMap<String, String>,
    references: &BTreeMap<String, String>,
) -> Result<(), ContractError> {
    for value in values.values_mut() {
        rewrite_expression_references(value, references)?;
    }
    Ok(())
}

pub(super) fn rewrite_condition_references(
    value: &mut String,
    references: &BTreeMap<String, String>,
) -> Result<(), ContractError> {
    *value = rewrite_expression_body(value, references)?;
    Ok(())
}

pub(super) fn rewrite_expression_references(
    value: &mut String,
    references: &BTreeMap<String, String>,
) -> Result<(), ContractError> {
    let mut rewritten = String::with_capacity(value.len());
    let mut remaining = value.as_str();
    while let Some(start) = remaining.find("${{") {
        let body_start = start + 3;
        let Some(body_end) = expression_end(remaining, body_start) else {
            break;
        };
        rewritten.push_str(&remaining[..body_start]);
        rewritten.push_str(&rewrite_expression_body(
            &remaining[body_start..body_end],
            references,
        )?);
        rewritten.push_str("}}");
        remaining = &remaining[body_end + 2..];
    }
    rewritten.push_str(remaining);
    *value = rewritten;
    Ok(())
}

fn expression_end(value: &str, start: usize) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut index = start;
    let mut quote = None;
    while index + 1 < bytes.len() {
        let current = bytes[index];
        if let Some(active_quote) = quote {
            if current == active_quote {
                if bytes.get(index + 1) == Some(&active_quote) {
                    index += 2;
                    continue;
                }
                quote = None;
            } else if active_quote == b'"' && current == b'\\' {
                index += 1;
            }
        } else if current == b'\'' || current == b'"' {
            quote = Some(current);
        } else if current == b'}' && bytes[index + 1] == b'}' {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn rewrite_expression_body(
    value: &str,
    references: &BTreeMap<String, String>,
) -> Result<String, ContractError> {
    let chars = value.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    let mut quote = None;
    while index < chars.len() {
        let current = chars[index];
        if let Some(active_quote) = quote {
            output.push(current);
            if current == active_quote {
                if chars.get(index + 1) == Some(&active_quote) {
                    output.push(active_quote);
                    index += 2;
                    continue;
                }
                quote = None;
            } else if active_quote == '"' && current == '\\' && index + 1 < chars.len() {
                index += 1;
                output.push(chars[index]);
            }
            index += 1;
            continue;
        }
        if current == '\'' || current == '"' {
            quote = Some(current);
            output.push(current);
            index += 1;
            continue;
        }
        let previous_is_identifier = chars[..index]
            .iter()
            .rev()
            .find(|character| !character.is_whitespace())
            .is_some_and(|character| {
                character.is_ascii_alphanumeric()
                    || *character == '_'
                    || *character == '-'
                    || *character == '.'
            });
        if !previous_is_identifier && chars[index..].starts_with(&['s', 't', 'e', 'p', 's', '.']) {
            let id_start = index + 6;
            let id_end = chars[id_start..]
                .iter()
                .position(|character| *character == '.')
                .map(|offset| id_start + offset);
            if let Some(id_end) = id_end {
                let step_id = chars[id_start..id_end].iter().collect::<String>();
                if let Some(replacement) = references.get(&step_id) {
                    output.push_str("steps.");
                    output.push_str(replacement);
                    output.push('.');
                    index = id_end + 1;
                    continue;
                }
                if step_id.starts_with("qualification-cache-") {
                    return Err(cache_owner_mismatch());
                }
            }
        }
        output.push(current);
        index += 1;
    }
    Ok(output)
}
