use crate::OrchestratorError;

use super::shellcheck_fail;

pub(super) fn validate_mapping_value(
    key: &str,
    value: &str,
    is_step_run: bool,
) -> Result<(), OrchestratorError> {
    if key == "<<" {
        return Err(shellcheck_fail("workflow_merge_key_unsupported"));
    }
    let key_property = starts_with_node_property(key.trim_start())
        || (starts_with_flow(key.trim_start()) && flow_contains_node_property(key.trim_start()));
    let value = value.trim_start();
    let node_property = starts_with_node_property(value);
    let flow_property = starts_with_flow(value) && flow_contains_node_property(value);
    if is_step_run {
        if flow_property {
            return Err(shellcheck_fail("run_scalar_alias_shape_unsupported"));
        }
        return Ok(());
    }
    if key_property || node_property || flow_property {
        return Err(shellcheck_fail("workflow_alias_outside_step_run"));
    }
    Ok(())
}

pub(super) fn reject_non_mapping_content(content: &str) -> Result<(), OrchestratorError> {
    let content = content.strip_prefix("- ").unwrap_or(content).trim_start();
    if starts_with_node_property(content)
        || (starts_with_flow(content) && flow_contains_node_property(content))
    {
        return Err(shellcheck_fail("workflow_alias_outside_step_run"));
    }
    Ok(())
}

fn starts_with_node_property(value: &str) -> bool {
    value.starts_with(['&', '*'])
}

fn starts_with_flow(value: &str) -> bool {
    value.starts_with(['[', '{'])
}

fn flow_contains_node_property(value: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_usize;
    let mut previous_significant = None;
    let mut previous_character = None;
    let mut characters = value.chars().peekable();

    while let Some(character) = characters.next() {
        if let Some(delimiter) = quote {
            if delimiter == '"' && escaped {
                escaped = false;
                continue;
            }
            if delimiter == '"' && character == '\\' {
                escaped = true;
                continue;
            }
            if delimiter == '\'' && character == '\'' && characters.peek() == Some(&'\'') {
                let _ = characters.next();
                continue;
            }
            if character == delimiter {
                quote = None;
            }
            continue;
        }

        if character == '#' && previous_character.is_none_or(char::is_whitespace) {
            break;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '[' | '{' => depth = depth.saturating_add(1),
            ']' | '}' => depth = depth.saturating_sub(1),
            '&' | '*' if depth > 0 && is_flow_node_boundary(previous_significant) => return true,
            _ => {}
        }
        if !character.is_whitespace() {
            previous_significant = Some(character);
        }
        previous_character = Some(character);
    }
    false
}

fn is_flow_node_boundary(previous: Option<char>) -> bool {
    matches!(previous, None | Some('[' | '{' | ',' | ':'))
}
