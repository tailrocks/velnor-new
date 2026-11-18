// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::Reason;
use serde::Deserialize;
use velnor_model::PrivateSources;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPrivateSources {
    app_id: String,
    private_key: String,
    repositories: Vec<String>,
}

pub(crate) fn private_sources(raw: RawPrivateSources) -> Result<PrivateSources, Reason> {
    let invalid = |key, value| Reason::InvalidValue { key, value };
    let app_id = context_name(&raw.app_id, "vars")
        .ok_or_else(|| invalid("private_sources.app_id", raw.app_id.clone()))?;
    let private_key = context_name(&raw.private_key, "secrets")
        .ok_or_else(|| invalid("private_sources.private_key", raw.private_key.clone()))?;
    let mut owner = None;
    let mut repositories = Vec::new();
    for entry in &raw.repositories {
        let (entry_owner, name) = entry
            .split_once('/')
            .filter(|(owner, name)| slug(owner) && slug(name))
            .ok_or_else(|| invalid("private_sources.repositories", entry.clone()))?;
        if *owner.get_or_insert(entry_owner) != entry_owner || repositories.contains(&name) {
            return Err(invalid("private_sources.repositories", entry.clone()));
        }
        repositories.push(name);
    }
    let owner = owner
        .ok_or_else(|| invalid("private_sources.repositories", "[]".to_owned()))?
        .to_owned();
    Ok(PrivateSources {
        app_id: app_id.to_owned(),
        private_key: private_key.to_owned(),
        owner,
        repositories: repositories.into_iter().map(str::to_owned).collect(),
    })
}

fn context_name<'a>(value: &'a str, context: &str) -> Option<&'a str> {
    value
        .strip_prefix(context)
        .and_then(|rest| rest.strip_prefix('.'))
        .filter(|name| {
            name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && name.chars().next().is_some_and(|c| !c.is_ascii_digit())
        })
}

fn slug(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('.')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}
