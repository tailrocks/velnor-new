//! Typed workflow header serialization.
use super::inputs::dispatch_input_to_yaml;
use crate::yaml::Yaml;
use velnor_actions_contract::{PermissionLevel, Permissions, Trigger};

/// YAML spelling of one contract permission level.
fn level_str(level: PermissionLevel) -> &'static str {
    match level {
        PermissionLevel::None => "none",
        PermissionLevel::Read => "read",
        PermissionLevel::Write => "write",
    }
}

/// Render permissions: contents/actions always, grants beyond none explicit.
///
/// The CI default stays exactly `contents: read` plus `actions: read`;
/// wider scopes render only when the IR grants them, so validated
/// overrides are never silently dropped.
pub(super) fn permissions_to_yaml(permissions: &Permissions) -> Yaml {
    let mut entries = vec![
        (
            "contents".to_owned(),
            Yaml::str(level_str(permissions.contents).to_owned()),
        ),
        (
            "actions".to_owned(),
            Yaml::str(level_str(permissions.actions).to_owned()),
        ),
    ];
    if !matches!(permissions.pull_requests, PermissionLevel::None) {
        entries.push((
            "pull-requests".to_owned(),
            Yaml::str(level_str(permissions.pull_requests).to_owned()),
        ));
    }
    if !matches!(permissions.id_token, PermissionLevel::None) {
        entries.push((
            "id-token".to_owned(),
            Yaml::str(level_str(permissions.id_token).to_owned()),
        ));
    }
    if !matches!(permissions.issues, PermissionLevel::None) {
        entries.push((
            "issues".to_owned(),
            Yaml::str(level_str(permissions.issues).to_owned()),
        ));
    }
    if !matches!(permissions.pages, PermissionLevel::None) {
        entries.push((
            "pages".to_owned(),
            Yaml::str(level_str(permissions.pages).to_owned()),
        ));
    }
    if !matches!(permissions.attestations, PermissionLevel::None) {
        entries.push((
            "attestations".to_owned(),
            Yaml::str(level_str(permissions.attestations).to_owned()),
        ));
    }
    Yaml::Map(entries)
}

/// Render only explicitly enabled typed events.
pub(super) fn triggers_to_yaml(triggers: &Trigger) -> Yaml {
    let mut entries = Vec::new();
    if !triggers.pull_request_types.is_empty() {
        entries.push((
            "pull_request".to_owned(),
            Yaml::Map(vec![(
                "types".to_owned(),
                Yaml::Seq(
                    triggers
                        .pull_request_types
                        .iter()
                        .cloned()
                        .map(Yaml::str)
                        .collect(),
                ),
            )]),
        ));
    }
    let mut push = Vec::new();
    for (key, values) in [
        ("branches", &triggers.push_branches),
        ("tags", &triggers.push_tags),
    ] {
        if !values.is_empty() {
            push.push((
                key.to_owned(),
                Yaml::Seq(values.iter().cloned().map(Yaml::str).collect()),
            ));
        }
    }
    if !push.is_empty() {
        entries.push(("push".to_owned(), Yaml::Map(push)));
    }
    if let Some(schedule) = &triggers.schedule {
        let crons = schedule
            .cron
            .iter()
            .map(|cron| Yaml::Map(vec![("cron".to_owned(), Yaml::str(cron.clone()))]))
            .collect();
        entries.push(("schedule".to_owned(), Yaml::Seq(crons)));
    }
    if let Some(dispatch) = &triggers.workflow_dispatch {
        let inputs = dispatch
            .inputs
            .iter()
            .map(|input| (input.name.clone(), dispatch_input_to_yaml(input)))
            .collect();
        entries.push((
            "workflow_dispatch".to_owned(),
            Yaml::Map(vec![("inputs".to_owned(), Yaml::Map(inputs))]),
        ));
    }
    if triggers.merge_group {
        entries.push(("merge_group".to_owned(), Yaml::Null));
    }
    Yaml::Map(entries)
}
