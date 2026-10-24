use std::collections::{BTreeMap, BTreeSet};

use saphyr::Yaml;

use crate::RenderError;

use super::{AuthorityConcurrency, get, invalid, scalar, untagged};

pub(super) fn permission_map(
    value: &Yaml<'_>,
    path: &str,
    key: &str,
) -> Result<BTreeMap<String, String>, RenderError> {
    let map = untagged(value)
        .as_mapping()
        .ok_or_else(|| invalid(format!("workflow_permissions_not_map:{path}:{key}")))?;
    let mut permissions = BTreeMap::new();
    for (name, level) in map {
        let name =
            scalar(name).ok_or_else(|| invalid(format!("permission_name_invalid:{path}:{key}")))?;
        let level = scalar(level)
            .ok_or_else(|| invalid(format!("permission_level_unresolved:{path}:{key}:{name}")))?;
        if !matches!(level, "read" | "write" | "none") {
            return Err(invalid(format!(
                "permission_level_invalid:{path}:{key}:{name}"
            )));
        }
        permissions.insert(name.to_owned(), level.to_owned());
    }
    Ok(permissions)
}

pub(super) fn concurrency(
    value: &Yaml<'_>,
    scope: &str,
    path: &str,
    key: &str,
) -> Result<AuthorityConcurrency, RenderError> {
    let group = get(value, "group")
        .and_then(scalar)
        .ok_or_else(|| invalid(format!("concurrency_group_unresolved:{path}:{key}")))?;
    let cancel = get(value, "cancel-in-progress")
        .ok_or_else(|| invalid(format!("concurrency_cancel_missing:{path}:{key}")))?;
    let cancel_in_progress = scalar(cancel)
        .map(str::to_owned)
        .or_else(|| cancel.as_bool().map(|value| value.to_string()))
        .ok_or_else(|| invalid(format!("concurrency_cancel_unresolved:{path}:{key}")))?;
    Ok(AuthorityConcurrency {
        scope: scope.to_owned(),
        group: group.to_owned(),
        cancel_in_progress,
    })
}

pub(super) fn extract_needs(
    value: Option<&Yaml<'_>>,
    path: &str,
    key: &str,
) -> Result<Vec<String>, RenderError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = if let Some(value) = scalar(value) {
        vec![value.to_owned()]
    } else {
        value
            .as_sequence()
            .ok_or_else(|| invalid(format!("workflow_needs_invalid:{path}:{key}")))?
            .iter()
            .map(|entry| scalar(entry).map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| invalid(format!("workflow_needs_invalid:{path}:{key}")))?
    };
    let mut unique = BTreeSet::new();
    if values.iter().any(|entry| !unique.insert(entry.clone())) {
        return Err(invalid(format!("workflow_needs_duplicate:{path}:{key}")));
    }
    Ok(unique.into_iter().collect())
}

pub(super) fn extract_environments(
    value: Option<&Yaml<'_>>,
    path: &str,
    key: &str,
) -> Result<Vec<String>, RenderError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let name = scalar(value).or_else(|| get(value, "name").and_then(scalar));
    let name = name.ok_or_else(|| invalid(format!("environment_name_unresolved:{path}:{key}")))?;
    if name.contains("${{") {
        return Err(invalid(format!("environment_name_unresolved:{path}:{key}")));
    }
    Ok(vec![name.to_owned()])
}

pub(super) fn collect_secrets(
    value: &Yaml<'_>,
    path: &str,
    key: &str,
) -> Result<Vec<String>, RenderError> {
    let mut strings = Vec::new();
    collect_scalars(value, &mut strings);
    let mut secrets = BTreeSet::new();
    if let Some(declarations) = get(value, "secrets") {
        if scalar(declarations) == Some("inherit") {
            return Err(invalid(format!("secret_inherit_unresolved:{path}:{key}")));
        }
        if let Some(map) = untagged(declarations).as_mapping() {
            for (name, _) in map {
                let name = scalar(name)
                    .ok_or_else(|| invalid(format!("secret_name_unresolved:{path}:{key}")))?;
                secrets.insert(name.to_owned());
            }
        }
    }
    for source in strings {
        let normalized = source.to_ascii_lowercase();
        if has_bare_secret_context(&normalized) {
            return Err(invalid(format!("secret_reference_unresolved:{path}:{key}")));
        }
        let mut offset = 0;
        while let Some(relative) = normalized[offset..].find("secrets.") {
            let start = offset + relative + "secrets.".len();
            let tail = &normalized[start..];
            let end = tail
                .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
                .unwrap_or(tail.len());
            if end == 0 {
                return Err(invalid(format!("secret_reference_unresolved:{path}:{key}")));
            }
            secrets.insert(source[start..start + end].to_owned());
            offset = start + end;
        }
        if normalized.contains("secrets[") {
            return Err(invalid(format!("secret_reference_unresolved:{path}:{key}")));
        }
    }
    Ok(secrets.into_iter().collect())
}

pub(super) fn merge_secrets(inherited: &[String], local: &[String]) -> Vec<String> {
    inherited
        .iter()
        .chain(local)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn has_bare_secret_context(value: &str) -> bool {
    value.match_indices("secrets").any(|(index, _)| {
        let before = value[..index].chars().next_back();
        let after = value[index + "secrets".len()..].chars().next();
        before.is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
            && after.is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
            && after != Some('.')
    })
}

fn collect_scalars<'a>(value: &'a Yaml<'_>, output: &mut Vec<&'a str>) {
    if let Some(value) = scalar(value) {
        output.push(value);
    } else if let Some(sequence) = value.as_sequence() {
        for entry in sequence {
            collect_scalars(entry, output);
        }
    } else if let Some(mapping) = value.as_mapping() {
        for (key, entry) in mapping {
            collect_scalars(key, output);
            collect_scalars(entry, output);
        }
    }
}

pub(super) fn optional_scalar(
    value: Option<&Yaml<'_>>,
    path: &str,
    key: &str,
    field: &str,
) -> Result<Option<String>, RenderError> {
    value
        .map(|value| {
            scalar(value)
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("{field}_unresolved:{path}:{key}")))
        })
        .transpose()
}
