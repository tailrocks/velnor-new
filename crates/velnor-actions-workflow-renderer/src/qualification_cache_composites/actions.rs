use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::{
    QualificationCacheCompositeId, QualificationCacheCompositePhase, QualificationCacheLayer,
};
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::document_lanes::JobStepContext;
use crate::tree::RenderedFile;
use crate::{RenderError, composite, marker, steps, yaml};

use super::{Output, PREFIX};

pub(super) fn call_step(
    job_id: &str,
    phase: QualificationCacheCompositePhase,
    layer: Option<QualificationCacheLayer>,
    id: QualificationCacheCompositeId,
    with: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    let name = match phase {
        QualificationCacheCompositePhase::Prepare => "Prepare qualification cache",
        QualificationCacheCompositePhase::BeforeSave => "Prepare qualification cache save",
        QualificationCacheCompositePhase::AfterSave => "Record qualification cache save",
    };
    let role = match phase {
        QualificationCacheCompositePhase::Prepare => StepRole::QualificationCachePrepareComposite,
        QualificationCacheCompositePhase::BeforeSave => {
            StepRole::QualificationCacheBeforeSaveComposite
        }
        QualificationCacheCompositePhase::AfterSave => {
            StepRole::QualificationCacheAfterSaveComposite
        }
    };
    let local_action = match (phase, layer) {
        (QualificationCacheCompositePhase::Prepare, None) => {
            format!("{PREFIX}prepare-{job_id}")
        }
        (QualificationCacheCompositePhase::BeforeSave, Some(layer)) => {
            format!("{PREFIX}before-save-{}-{job_id}", layer.as_str())
        }
        (QualificationCacheCompositePhase::AfterSave, Some(layer)) => {
            format!("{PREFIX}after-save-{}-{job_id}", layer.as_str())
        }
        _ => {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_composite_scope_invalid".to_owned(),
            ));
        }
    };
    let step = Step {
        name: name.to_owned(),
        id: Some(StepId::QualificationCacheComposite(id)),
        role: Some(role),
        condition: Some("needs.plan.outputs.qualification_campaign != ''".to_owned()),
        parallel: false,
        kind: StepKind::Action {
            uses: local_action,
            with,
            env: BTreeMap::new(),
        },
    };
    velnor_actions_contract::workflow::step_identity::validate_step_sequence(
        std::slice::from_ref(&step),
        job_id,
    )
    .map_err(RenderError::Contract)?;
    Ok(step)
}

pub(super) fn composite_file(
    job_id: &str,
    phase: QualificationCacheCompositePhase,
    layer: Option<QualificationCacheLayer>,
    mut steps: Vec<Step>,
    input_names: Vec<String>,
    outputs: &[Output],
    ctx: &crate::render::RenderContext,
) -> Result<RenderedFile, RenderError> {
    let logical = match (phase, layer) {
        (QualificationCacheCompositePhase::Prepare, None) => {
            format!("qualification-cache-prepare-{job_id}")
        }
        (QualificationCacheCompositePhase::BeforeSave, Some(layer)) => {
            format!(
                "qualification-cache-before-save-{}-{job_id}",
                layer.as_str()
            )
        }
        (QualificationCacheCompositePhase::AfterSave, Some(layer)) => {
            format!("qualification-cache-after-save-{}-{job_id}", layer.as_str())
        }
        _ => {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_composite_scope_invalid".to_owned(),
            ));
        }
    };
    velnor_actions_contract::workflow::step_identity::validate_step_identity_scope(&steps, job_id)
        .map_err(RenderError::Contract)?;
    let rendered = render_composite_steps(job_id, &mut steps, ctx)?;
    let declared_inputs = input_declarations(input_names);
    let declared_outputs = output_declarations(outputs);
    let body = composite::composite_yaml_with_inputs_and_outputs(
        &logical,
        declared_inputs,
        declared_outputs,
        rendered,
    )?;
    validate_action_metadata(&body)?;
    let bytes = marker::with_marker(&ctx.generator_version, &yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{logical}/action.yml"),
        bytes,
    })
}

fn render_composite_steps(
    job_id: &str,
    steps: &mut [Step],
    ctx: &crate::render::RenderContext,
) -> Result<Vec<yaml::Yaml>, RenderError> {
    for step in steps.iter_mut() {
        if step
            .condition
            .as_deref()
            .is_some_and(contains_status_function)
        {
            return Err(RenderError::InvalidWorkflow(
                "qualification_cache_composite_status_function_invalid".to_owned(),
            ));
        }
        if let Some(condition) = &mut step.condition {
            *condition = condition.replace(
                "needs.plan.outputs.qualification_campaign",
                "inputs.qualification_campaign",
            );
        }
    }
    let mut rendered = Vec::with_capacity(steps.len());
    let empty_job_env = BTreeMap::new();
    let step_context = JobStepContext {
        job_env: &empty_job_env,
        runs_on: None,
        actions_read: false,
    };
    for step in steps.iter() {
        rendered.push(crate::document_steps::step_to_yaml(
            job_id,
            step,
            ctx,
            &[],
            true,
            &step_context,
        )?);
    }
    validate_composite_condition_references(&rendered)?;
    Ok(rendered)
}

fn input_declarations(input_names: Vec<String>) -> Vec<(String, yaml::Yaml)> {
    let mut input_names = input_names;
    input_names.sort();
    input_names.dedup();
    input_names
        .into_iter()
        .map(|name| {
            (
                name,
                yaml::Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        yaml::Yaml::str("Validated cache workflow value"),
                    ),
                    ("required".to_owned(), yaml::Yaml::Bool(true)),
                ]),
            )
        })
        .collect()
}

fn contains_status_function(condition: &str) -> bool {
    ["always", "cancelled", "failure", "success"]
        .into_iter()
        .any(|name| {
            condition
                .to_ascii_lowercase()
                .match_indices(name)
                .any(|(start, _)| {
                    let before = condition[..start].chars().next_back();
                    let after = condition[start + name.len()..].trim_start();
                    !before.is_some_and(|character| {
                        character.is_ascii_alphanumeric() || character == '_'
                    }) && after.starts_with('(')
                })
        })
}

fn output_declarations(outputs: &[Output]) -> Vec<(String, yaml::Yaml)> {
    outputs
        .iter()
        .map(|output| {
            (
                output.name.clone(),
                yaml::Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        yaml::Yaml::str("Prepared cache lifecycle output"),
                    ),
                    (
                        "value".to_owned(),
                        yaml::Yaml::str(format!(
                            "${{{{ steps.{}.outputs.{} }}}}",
                            output.id.as_string(),
                            output.field
                        )),
                    ),
                ]),
            )
        })
        .collect()
}

fn validate_action_metadata(action: &yaml::Yaml) -> Result<(), RenderError> {
    let invalid = || {
        RenderError::InvalidWorkflow("qualification_cache_composite_metadata_invalid".to_owned())
    };
    let yaml::Yaml::Map(action_fields) = action else {
        return Err(invalid());
    };
    let runs = action_fields
        .iter()
        .find_map(|(key, value)| (key == "runs").then_some(value))
        .ok_or_else(invalid)?;
    let yaml::Yaml::Map(run_fields) = runs else {
        return Err(invalid());
    };
    if !run_fields.iter().any(|(key, value)| {
        key == "using" && matches!(value, yaml::Yaml::Str(using) if using == "composite")
    }) {
        return Err(invalid());
    }
    let steps = run_fields
        .iter()
        .find_map(|(key, value)| (key == "steps").then_some(value))
        .ok_or_else(invalid)?;
    let yaml::Yaml::Seq(steps) = steps else {
        return Err(invalid());
    };
    for step in steps {
        let yaml::Yaml::Map(fields) = step else {
            return Err(invalid());
        };
        let has_run = fields.iter().any(|(key, _)| key == "run");
        let has_uses = fields.iter().any(|(key, _)| key == "uses");
        let allowed: &[&str] = if has_run {
            &[
                "name",
                "id",
                "if",
                "run",
                "env",
                "continue-on-error",
                "working-directory",
                "shell",
            ]
        } else {
            &[
                "name",
                "id",
                "if",
                "uses",
                "continue-on-error",
                "with",
                "env",
            ]
        };
        if has_run == has_uses
            || fields
                .iter()
                .any(|(key, _)| !allowed.contains(&key.as_str()))
            || (has_run && !fields.iter().any(|(key, _)| key == "shell"))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn validate_composite_condition_references(steps: &[yaml::Yaml]) -> Result<(), RenderError> {
    let local_ids: BTreeSet<&str> = steps
        .iter()
        .filter_map(|step| match step {
            yaml::Yaml::Map(entries) => match yaml_field(entries, "id") {
                Some(yaml::Yaml::Str(id)) => Some(id.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    for step in steps {
        let yaml::Yaml::Map(entries) = step else {
            continue;
        };
        let Some(yaml::Yaml::Str(condition)) = yaml_field(entries, "if") else {
            continue;
        };
        let mut remaining = condition.as_str();
        while let Some((_, after)) = remaining.split_once("steps.") {
            let id = after.split('.').next().unwrap_or_default();
            if id.is_empty() || !local_ids.contains(id) {
                return Err(RenderError::InvalidWorkflow(
                    "qualification_cache_composite_condition_reference_invalid".to_owned(),
                ));
            }
            remaining = after;
        }
    }
    Ok(())
}

fn yaml_field<'a>(entries: &'a [(String, yaml::Yaml)], name: &str) -> Option<&'a yaml::Yaml> {
    entries
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}

#[cfg(test)]
mod tests {
    use crate::yaml::Yaml;

    use super::{contains_status_function, validate_action_metadata};

    fn action(step: Vec<(String, Yaml)>) -> Yaml {
        Yaml::Map(vec![(
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite")),
                ("steps".to_owned(), Yaml::Seq(vec![Yaml::Map(step)])),
            ]),
        )])
    }

    #[test]
    fn composite_metadata_requires_supported_child_shapes() {
        assert!(
            validate_action_metadata(&action(vec![
                ("run".to_owned(), Yaml::str("true")),
                ("shell".to_owned(), Yaml::str("bash")),
            ]))
            .is_ok()
        );
        assert!(
            validate_action_metadata(&action(vec![(
                "uses".to_owned(),
                Yaml::str("actions/cache/save@v4")
            ),]))
            .is_ok()
        );
        assert!(
            validate_action_metadata(&action(vec![("run".to_owned(), Yaml::str("true")),]))
                .is_err()
        );
        assert!(
            validate_action_metadata(&action(vec![
                ("uses".to_owned(), Yaml::str("actions/cache/save@v4")),
                ("timeout-minutes".to_owned(), Yaml::Int(1)),
            ]))
            .is_err()
        );
        assert!(
            validate_action_metadata(&action(vec![
                ("run".to_owned(), Yaml::str("true")),
                ("shell".to_owned(), Yaml::str("bash")),
                ("uses".to_owned(), Yaml::str("actions/cache/save@v4")),
            ]))
            .is_err()
        );
    }

    #[test]
    fn composite_steps_reject_status_functions_that_change_outer_success_gates() {
        for condition in [
            "always()",
            "failure() && inputs.active == 'true'",
            "cancelled ()",
            "success()",
        ] {
            assert!(contains_status_function(condition), "{condition}");
        }
        for condition in [
            "inputs.qualification_campaign != ''",
            "steps.cache.outputs.active == 'true'",
            "not_success()",
        ] {
            assert!(!contains_status_function(condition), "{condition}");
        }
    }
}
