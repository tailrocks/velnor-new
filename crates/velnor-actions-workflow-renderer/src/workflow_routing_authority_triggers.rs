use saphyr::Yaml;

use crate::RenderError;

use super::{AuthorityTrigger, get, invalid, scalar, untagged};

pub(super) fn extract_triggers(
    path: &str,
    on: Option<&Yaml<'_>>,
) -> Result<Vec<AuthorityTrigger>, RenderError> {
    let on = on.ok_or_else(|| invalid(format!("workflow_triggers_missing:{path}")))?;
    if let Some(events) = untagged(on).as_sequence() {
        return sequence_triggers(path, events);
    }
    if let Some(event) = scalar(on) {
        if event == "workflow_call" {
            return Ok(Vec::new());
        }
        if !supported_event(event) {
            return Err(invalid(format!(
                "workflow_event_unsupported:{path}:{event}"
            )));
        }
        return Ok(vec![AuthorityTrigger {
            event: event.to_owned(),
            branch_filters: Vec::new(),
            tag_filters: Vec::new(),
            pull_request_base_branches: Vec::new(),
        }]);
    }
    let Some(events) = untagged(on).as_mapping() else {
        return Err(invalid(format!("workflow_triggers_not_map:{path}")));
    };
    map_triggers(path, events.iter())
}

fn sequence_triggers(
    path: &str,
    events: &[Yaml<'_>],
) -> Result<Vec<AuthorityTrigger>, RenderError> {
    let mut output = Vec::new();
    for value in events {
        let event =
            scalar(value).ok_or_else(|| invalid(format!("workflow_event_not_scalar:{path}")))?;
        if event != "workflow_call" {
            output.push(trigger_without_filters(event, path)?);
        }
    }
    output.sort_by(|left, right| left.event.cmp(&right.event));
    Ok(output)
}

fn map_triggers<'a>(
    path: &str,
    events: impl Iterator<Item = (&'a Yaml<'a>, &'a Yaml<'a>)>,
) -> Result<Vec<AuthorityTrigger>, RenderError> {
    let mut output = Vec::new();
    for (event_value, options) in events {
        let event = scalar(event_value)
            .ok_or_else(|| invalid(format!("workflow_event_not_scalar:{path}")))?;
        if event == "workflow_call" {
            continue;
        }
        validate_event(event, path)?;
        if has(options, "branches-ignore") || has(options, "tags-ignore") {
            return Err(invalid(format!(
                "workflow_ignore_filter_unsupported:{path}:{event}"
            )));
        }
        let branches = exact_filter(get(options, "branches"), path, event, "branches")?;
        let tags = exact_filter(get(options, "tags"), path, event, "tags")?;
        let is_pull_request = matches!(event, "pull_request" | "pull_request_target");
        let bases = if is_pull_request {
            exact_filter(
                get(options, "branches"),
                path,
                event,
                "pull_request_base_branches",
            )?
        } else {
            Vec::new()
        };
        output.push(AuthorityTrigger {
            event: event.to_owned(),
            branch_filters: if is_pull_request {
                Vec::new()
            } else {
                branches
            },
            tag_filters: tags,
            pull_request_base_branches: bases,
        });
    }
    output.sort_by(|left, right| left.event.cmp(&right.event));
    Ok(output)
}

fn trigger_without_filters(event: &str, path: &str) -> Result<AuthorityTrigger, RenderError> {
    validate_event(event, path)?;
    Ok(AuthorityTrigger {
        event: event.to_owned(),
        branch_filters: Vec::new(),
        tag_filters: Vec::new(),
        pull_request_base_branches: Vec::new(),
    })
}

fn validate_event(event: &str, path: &str) -> Result<(), RenderError> {
    if supported_event(event) {
        Ok(())
    } else {
        Err(invalid(format!(
            "workflow_event_unsupported:{path}:{event}"
        )))
    }
}

fn supported_event(event: &str) -> bool {
    matches!(
        event,
        "push"
            | "pull_request"
            | "pull_request_target"
            | "issue_comment"
            | "merge_group"
            | "schedule"
            | "workflow_dispatch"
            | "release"
    )
}

fn exact_filter(
    value: Option<&Yaml<'_>>,
    path: &str,
    event: &str,
    field: &str,
) -> Result<Vec<String>, RenderError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let list = untagged(value)
        .as_sequence()
        .ok_or_else(|| invalid(format!("workflow_filter_not_list:{path}:{event}:{field}")))?;
    if list.is_empty() {
        return Err(invalid(format!(
            "workflow_filter_empty:{path}:{event}:{field}"
        )));
    }
    let mut values = Vec::new();
    for entry in list {
        let entry = scalar(entry)
            .ok_or_else(|| invalid(format!("workflow_filter_not_scalar:{path}:{event}:{field}")))?;
        if entry.is_empty()
            || entry
                .bytes()
                .any(|byte| matches!(byte, b'*' | b'+' | b'?' | b'[' | b']' | b'!'))
        {
            return Err(invalid(format!(
                "workflow_filter_not_exact:{path}:{event}:{field}"
            )));
        }
        values.push(entry.to_owned());
    }
    values.sort();
    values.dedup();
    Ok(values)
}

fn has(value: &Yaml<'_>, name: &str) -> bool {
    untagged(value)
        .as_mapping()
        .is_some_and(|entries| entries.iter().any(|(key, _)| scalar(key) == Some(name)))
}

#[cfg(test)]
#[path = "workflow_routing_authority_triggers_tests.rs"]
mod tests;
