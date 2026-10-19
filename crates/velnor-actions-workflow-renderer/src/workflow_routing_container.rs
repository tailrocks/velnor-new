use std::collections::BTreeSet;

use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::config::ValidatedImageReference;

const CONTAINER_FIELDS: [&str; 6] = ["image", "credentials", "env", "ports", "volumes", "options"];

pub(super) fn ensure_job_container(
    fields: &mut Vec<(String, Yaml)>,
    image: Option<&ValidatedImageReference>,
    workflow_shell: Option<&str>,
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let image =
        image.ok_or_else(|| invalid(format!("job_container_image_missing:{path}:{job_id}")))?;
    let existing = fields.iter().position(|(key, _)| key == "container");
    if let Some(index) = existing {
        validate_container(&fields[index].1, path, job_id)?;
    } else {
        fields.push((
            "container".to_owned(),
            Yaml::Map(vec![("image".to_owned(), Yaml::str(image.as_str()))]),
        ));
    }
    if existing.is_none() {
        ensure_container_shell(fields, workflow_shell, path, job_id)?;
    }
    Ok(())
}

pub(super) fn workflow_run_shell(
    workflow: &Yaml,
    path: &str,
) -> Result<Option<String>, RenderError> {
    let Yaml::Map(document) = workflow else {
        return Err(invalid(format!("workflow_document_not_map:{path}")));
    };
    let Some(defaults) = document
        .iter()
        .find_map(|(key, value)| (key == "defaults").then_some(value))
    else {
        return Ok(None);
    };
    let Yaml::Map(defaults) = defaults else {
        return Err(invalid(format!("workflow_defaults_invalid:{path}")));
    };
    let Some(run) = defaults
        .iter()
        .find_map(|(key, value)| (key == "run").then_some(value))
    else {
        return Ok(None);
    };
    let Yaml::Map(run) = run else {
        return Err(invalid(format!("workflow_run_defaults_invalid:{path}")));
    };
    let Some(shell) = run
        .iter()
        .find_map(|(key, value)| (key == "shell").then_some(value))
    else {
        return Ok(None);
    };
    scalar_text(shell)
        .map(str::to_owned)
        .map(Some)
        .ok_or_else(|| invalid(format!("workflow_run_shell_invalid:{path}")))
}

pub(super) fn validate_parsed_container(
    value: &saphyr::Yaml<'_>,
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let value = untagged(value);
    match value {
        saphyr::Yaml::Value(_) | saphyr::Yaml::Representation(..) => {
            let image = value
                .as_str()
                .ok_or_else(|| invalid(format!("container_shape_invalid:{path}:{job_id}")))?;
            validate_image(image, path, job_id)
        }
        saphyr::Yaml::Mapping(entries) => {
            let mut image = None;
            let mut seen = BTreeSet::new();
            for (key, value) in entries {
                let key = key
                    .as_str()
                    .ok_or_else(|| invalid(format!("container_field_invalid:{path}:{job_id}")))?;
                if !CONTAINER_FIELDS.contains(&key) {
                    return Err(invalid(format!(
                        "unsupported_container_option:{path}:{job_id}:{key}"
                    )));
                }
                if !seen.insert(key) {
                    return Err(invalid(format!(
                        "container_field_duplicate:{path}:{job_id}:{key}"
                    )));
                }
                if key == "image" {
                    image = value.as_str();
                } else {
                    validate_parsed_option(key, value, path, job_id)?;
                }
            }
            let image =
                image.ok_or_else(|| invalid(format!("container_image_missing:{path}:{job_id}")))?;
            validate_image(image, path, job_id)
        }
        _ => Err(invalid(format!("container_shape_invalid:{path}:{job_id}"))),
    }
}

fn validate_container(value: &Yaml, path: &str, job_id: &str) -> Result<(), RenderError> {
    match value {
        Yaml::Str(image) | Yaml::Quoted(image) | Yaml::AnchoredScalar { value: image, .. } => {
            validate_image(image, path, job_id)
        }
        Yaml::Annotated { value: image, .. } => validate_image(image, path, job_id),
        Yaml::Map(entries) | Yaml::AnchoredMap { entries, .. } => {
            let mut image = None;
            let mut seen = BTreeSet::new();
            for (key, value) in entries {
                if !CONTAINER_FIELDS.contains(&key.as_str()) {
                    return Err(invalid(format!(
                        "unsupported_container_option:{path}:{job_id}:{key}"
                    )));
                }
                if !seen.insert(key.as_str()) {
                    return Err(invalid(format!(
                        "container_field_duplicate:{path}:{job_id}:{key}"
                    )));
                }
                if key == "image" {
                    if image.is_some() {
                        return Err(invalid(format!(
                            "container_image_duplicate:{path}:{job_id}"
                        )));
                    }
                    image = scalar_text(value);
                } else {
                    validate_option(key, value, path, job_id)?;
                }
            }
            let image =
                image.ok_or_else(|| invalid(format!("container_image_missing:{path}:{job_id}")))?;
            validate_image(image, path, job_id)
        }
        _ => Err(invalid(format!("container_shape_invalid:{path}:{job_id}"))),
    }
}

fn validate_option(key: &str, value: &Yaml, path: &str, job_id: &str) -> Result<(), RenderError> {
    let valid = match key {
        "credentials" | "env" => matches!(value, Yaml::Map(_) | Yaml::FlowMap(_)),
        "ports" | "volumes" => match value {
            Yaml::Seq(values) => values
                .iter()
                .all(|value| scalar_text(value).is_some() || matches!(value, Yaml::Int(_))),
            Yaml::Flow(values) => !values.is_empty(),
            _ => false,
        },
        "options" => scalar_text(value).is_some(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(format!(
            "container_option_invalid:{path}:{job_id}:{key}"
        )))
    }
}

fn validate_parsed_option(
    key: &str,
    value: &saphyr::Yaml<'_>,
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let value = untagged(value);
    let valid = match key {
        "credentials" | "env" => matches!(value, saphyr::Yaml::Mapping(_)),
        "ports" | "volumes" => value.as_vec().is_some_and(|values| {
            values
                .iter()
                .all(|value| value.as_str().is_some() || value.as_integer().is_some())
        }),
        "options" => value.as_str().is_some(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(format!(
            "container_option_invalid:{path}:{job_id}:{key}"
        )))
    }
}

fn validate_image(image: &str, path: &str, job_id: &str) -> Result<(), RenderError> {
    ValidatedImageReference::parse(image)
        .map(|_| ())
        .map_err(|_| invalid(format!("container_image_invalid:{path}:{job_id}")))
}

fn ensure_container_shell(
    fields: &mut Vec<(String, Yaml)>,
    workflow_shell: Option<&str>,
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let shell = workflow_shell.unwrap_or("bash -e {0}");
    let Some((_, defaults)) = fields.iter_mut().find(|(key, _)| key == "defaults") else {
        fields.push((
            "defaults".to_owned(),
            Yaml::Map(vec![(
                "run".to_owned(),
                Yaml::Map(vec![("shell".to_owned(), Yaml::str(shell))]),
            )]),
        ));
        return Ok(());
    };
    let Yaml::Map(defaults) = defaults else {
        return Err(invalid(format!("job_defaults_invalid:{path}:{job_id}")));
    };
    reject_duplicate_keys(defaults, path, job_id, "defaults")?;
    let Some((_, run)) = defaults.iter_mut().find(|(key, _)| key == "run") else {
        defaults.push((
            "run".to_owned(),
            Yaml::Map(vec![("shell".to_owned(), Yaml::str(shell))]),
        ));
        return Ok(());
    };
    let Yaml::Map(run) = run else {
        return Err(invalid(format!("job_run_defaults_invalid:{path}:{job_id}")));
    };
    reject_duplicate_keys(run, path, job_id, "run_defaults")?;
    if !run.iter().any(|(key, _)| key == "shell") {
        run.push(("shell".to_owned(), Yaml::str(shell)));
    }
    Ok(())
}

fn reject_duplicate_keys(
    fields: &[(String, Yaml)],
    path: &str,
    job_id: &str,
    owner: &str,
) -> Result<(), RenderError> {
    let mut seen = BTreeSet::new();
    for (key, _) in fields {
        if !seen.insert(key.as_str()) {
            return Err(invalid(format!(
                "workflow_field_duplicate:{path}:{job_id}:{owner}:{key}"
            )));
        }
    }
    Ok(())
}

fn scalar_text(value: &Yaml) -> Option<&str> {
    match value {
        Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. } => Some(value),
        _ => None,
    }
}

fn untagged<'a, 'input>(value: &'a saphyr::Yaml<'input>) -> &'a saphyr::Yaml<'input> {
    match value {
        saphyr::Yaml::Tagged(_, inner) => untagged(inner),
        _ => value,
    }
}

fn invalid(problem: String) -> RenderError {
    RenderError::InvalidWorkflow(problem)
}
