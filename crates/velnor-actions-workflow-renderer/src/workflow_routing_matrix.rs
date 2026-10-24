use crate::{RenderError, yaml::Yaml};

use super::{MAX_MATRIX_COMBINATIONS, invalid};

pub(super) fn matrix_runner_labels(
    fields: &[(String, Yaml)],
    expression: &str,
    path: &str,
    job_id: &str,
) -> Result<Vec<String>, RenderError> {
    let matrix = fields
        .iter()
        .find(|(name, _)| name == "strategy")
        .and_then(|(_, value)| match value {
            Yaml::Map(entries) => entries
                .iter()
                .find(|(name, _)| name == "matrix")
                .map(|(_, value)| value),
            _ => None,
        })
        .and_then(|value| match value {
            Yaml::Map(entries) => Some(entries.as_slice()),
            _ => None,
        })
        .ok_or_else(|| {
            invalid(format!(
                "matrix_runner_values_missing:{path}:{job_id}:{expression}"
            ))
        })?;
    let expression_path = matrix_expression_path(expression)
        .ok_or_else(|| invalid(format!("matrix_runner_expression_invalid:{path}:{job_id}")))?;
    let combinations = expand_matrix(matrix, path, job_id)?;
    let labels = combinations
        .iter()
        .map(|combination| {
            let value = matrix_value(combination, &expression_path).map_err(|_| {
                invalid(format!(
                    "matrix_runner_value_missing:{path}:{job_id}:{expression}"
                ))
            })?;
            match &value {
                Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. } => {
                    Ok(value.clone())
                }
                _ => Err(invalid(format!(
                    "matrix_runner_value_invalid:{path}:{job_id}:{expression}"
                ))),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if labels.is_empty() {
        return Err(invalid(format!(
            "matrix_runner_values_missing:{path}:{job_id}:{expression}"
        )));
    }
    Ok(labels)
}

fn expand_matrix(
    matrix: &[(String, Yaml)],
    path: &str,
    job_id: &str,
) -> Result<Vec<std::collections::BTreeMap<String, Yaml>>, RenderError> {
    let mut axes = Vec::new();
    let mut matrix_names = std::collections::BTreeSet::new();
    for (name, values) in matrix {
        if !matrix_names.insert(name.as_str()) {
            return Err(invalid(format!(
                "matrix_key_duplicate:{path}:{job_id}:{name}"
            )));
        }
        if matches!(name.as_str(), "include" | "exclude") {
            continue;
        }
        if axes.len() == MAX_MATRIX_COMBINATIONS {
            return Err(invalid(format!("matrix_too_large:{path}:{job_id}")));
        }
        axes.push((name, matrix_values(values, path, job_id, name)?));
    }
    let mut combinations = vec![std::collections::BTreeMap::new()];
    for (name, values) in &axes {
        let count = combinations
            .len()
            .checked_mul(values.len())
            .ok_or_else(|| invalid(format!("matrix_too_large:{path}:{job_id}")))?;
        if count > MAX_MATRIX_COMBINATIONS {
            return Err(invalid(format!("matrix_too_large:{path}:{job_id}")));
        }
        let mut expanded = Vec::with_capacity(count);
        for combination in &combinations {
            for value in values {
                let mut expanded_row = combination.clone();
                expanded_row.insert((*name).clone(), value.clone());
                expanded.push(expanded_row);
            }
        }
        combinations = expanded;
    }
    if let Some(exclude) = matrix.iter().find(|(name, _)| name == "exclude") {
        let exclusions = matrix_objects(&exclude.1, path, job_id, "exclude")?;
        combinations.retain(|combination| {
            !exclusions.iter().any(|exclusion| {
                exclusion.iter().all(|(name, value)| {
                    combination
                        .get(name)
                        .is_some_and(|candidate| candidate == value)
                })
            })
        });
    }
    if let Some(include) = matrix.iter().find(|(name, _)| name == "include") {
        let includes = matrix_objects(&include.1, path, job_id, "include")?;
        combinations = apply_includes(combinations, &axes, includes, path, job_id)?;
    }
    Ok(combinations)
}

fn apply_includes(
    mut combinations: Vec<std::collections::BTreeMap<String, Yaml>>,
    axes: &[(&String, Vec<Yaml>)],
    includes: Vec<std::collections::BTreeMap<String, Yaml>>,
    path: &str,
    job_id: &str,
) -> Result<Vec<std::collections::BTreeMap<String, Yaml>>, RenderError> {
    if axes.is_empty() {
        combinations.clear();
        combinations.extend(includes);
        return Ok(combinations);
    }
    let original_rows = combinations.clone();
    for include in includes {
        let mut merged = false;
        for (index, original) in original_rows.iter().enumerate() {
            let conflicts_with_axis = axes.iter().any(|(name, _)| {
                include.get(*name).is_some_and(|value| {
                    original
                        .get(*name)
                        .is_some_and(|axis_value| axis_value != value)
                })
            });
            if !conflicts_with_axis {
                combinations[index].extend(include.clone());
                merged = true;
            }
        }
        if !merged {
            combinations.push(include);
        }
        if combinations.len() > MAX_MATRIX_COMBINATIONS {
            return Err(invalid(format!("matrix_too_large:{path}:{job_id}")));
        }
    }
    Ok(combinations)
}

fn matrix_values(
    value: &Yaml,
    path: &str,
    job_id: &str,
    key: &str,
) -> Result<Vec<Yaml>, RenderError> {
    match value {
        Yaml::Seq(values) if values.len() <= MAX_MATRIX_COMBINATIONS => Ok(values.clone()),
        Yaml::Flow(values) if values.len() <= MAX_MATRIX_COMBINATIONS => {
            Ok(values.iter().cloned().map(Yaml::Str).collect())
        }
        Yaml::Seq(_) | Yaml::Flow(_) => Err(invalid(format!("matrix_too_large:{path}:{job_id}"))),
        _ => Err(invalid(format!(
            "matrix_axis_values_invalid:{path}:{job_id}:{key}"
        ))),
    }
}

fn matrix_objects(
    value: &Yaml,
    path: &str,
    job_id: &str,
    kind: &str,
) -> Result<Vec<std::collections::BTreeMap<String, Yaml>>, RenderError> {
    let rows = match value {
        Yaml::Seq(rows) if rows.len() <= MAX_MATRIX_COMBINATIONS => rows,
        Yaml::Seq(_) => return Err(invalid(format!("matrix_too_large:{path}:{job_id}"))),
        _ => return Err(invalid(format!("matrix_{kind}_invalid:{path}:{job_id}"))),
    };
    rows.iter()
        .map(|row| match row {
            Yaml::Map(entries) => matrix_object(entries, path, job_id, kind),
            Yaml::FlowMap(entries) => {
                let fields = entries
                    .iter()
                    .map(|(key, value)| (key.clone(), Yaml::Str(value.clone())))
                    .collect::<Vec<_>>();
                matrix_object(&fields, path, job_id, kind)
            }
            _ => Err(invalid(format!("matrix_{kind}_invalid:{path}:{job_id}"))),
        })
        .collect()
}

fn matrix_object(
    fields: &[(String, Yaml)],
    path: &str,
    job_id: &str,
    kind: &str,
) -> Result<std::collections::BTreeMap<String, Yaml>, RenderError> {
    if fields.len() > MAX_MATRIX_COMBINATIONS {
        return Err(invalid(format!("matrix_too_large:{path}:{job_id}")));
    }
    let mut object = std::collections::BTreeMap::new();
    for (key, value) in fields {
        if object.insert(key.clone(), value.clone()).is_some() {
            return Err(invalid(format!(
                "matrix_{kind}_duplicate_key:{path}:{job_id}:{key}"
            )));
        }
    }
    Ok(object)
}

fn matrix_expression_path(value: &str) -> Option<Vec<&str>> {
    let path = value.strip_prefix("${{ matrix.")?.strip_suffix(" }}")?;
    let parts: Vec<_> = path.split('.').collect();
    (!parts.is_empty()
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }))
    .then_some(parts)
}

pub(super) fn is_matrix_expression(value: &str) -> bool {
    matrix_expression_path(value).is_some()
}

fn matrix_value(
    combination: &std::collections::BTreeMap<String, Yaml>,
    path: &[&str],
) -> Result<Yaml, RenderError> {
    let mut value = combination
        .get(path.first().copied().unwrap_or_default())
        .cloned()
        .ok_or_else(|| invalid("matrix_runner_value_missing".to_owned()))?;
    for key in path.iter().skip(1) {
        value = match &value {
            Yaml::Map(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
                .ok_or_else(|| invalid("matrix_runner_value_missing".to_owned()))?,
            Yaml::FlowMap(entries) => {
                let (_, nested) = entries
                    .iter()
                    .find(|(name, _)| name == key)
                    .ok_or_else(|| invalid("matrix_runner_value_missing".to_owned()))?;
                Yaml::Str(nested.clone())
            }
            _ => return Err(invalid("matrix_runner_value_missing".to_owned())),
        };
    }
    Ok(value)
}

#[cfg(test)]
#[path = "workflow_routing_matrix_tests.rs"]
mod tests;
