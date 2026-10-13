use crate::OrchestratorError;

use super::shellcheck_fail;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MappingProperty {
    Anchor { name: String, inline_empty: bool },
    Alias(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StepProperty {
    Anchor(String),
    Alias(String),
}

pub(super) fn validate_mapping_value(
    key: &str,
    value: &str,
    is_step_run: bool,
    is_supported_mapping: bool,
) -> Result<(), OrchestratorError> {
    if key == "<<" {
        return Err(shellcheck_fail("workflow_merge_key_unsupported"));
    }
    let value = value.trim_start();
    if is_step_run {
        if starts_with_tag_property(value) {
            return Err(shellcheck_fail("run_scalar_tag_unsupported"));
        }
        if flow_value_after_tags(value).is_some_and(flow_contains_node_property) {
            return Err(shellcheck_fail("run_scalar_alias_shape_unsupported"));
        }
        return Ok(());
    }
    if is_supported_mapping {
        if parse_mapping_property(value)?.is_some() {
            if has_node_property(key) {
                return Err(shellcheck_fail("workflow_alias_outside_supported_mapping"));
            }
            return Ok(());
        }
        if starts_with_tag_property(value) || has_node_property(key) || has_node_property(value) {
            return Err(shellcheck_fail("workflow_alias_outside_step_run"));
        }
        if flow_value_after_tags(value).is_some_and(flow_contains_merge_key) {
            return Err(shellcheck_fail("workflow_merge_key_unsupported"));
        }
        return Ok(());
    }
    if has_node_property(key) || has_node_property(value) {
        return Err(shellcheck_fail("workflow_alias_outside_step_run"));
    }
    if flow_value_after_tags(key).is_some_and(flow_contains_merge_key)
        || flow_value_after_tags(value).is_some_and(flow_contains_merge_key)
    {
        return Err(shellcheck_fail("workflow_merge_key_unsupported"));
    }
    Ok(())
}

pub(super) fn parse_mapping_property(
    value: &str,
) -> Result<Option<MappingProperty>, OrchestratorError> {
    let value = value.trim();
    let Some(property) = value.as_bytes().first().copied() else {
        return Ok(None);
    };
    if !matches!(property, b'&' | b'*') {
        return Ok(None);
    }
    let rest = &value[1..];
    let name_len = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        .count();
    if name_len == 0 {
        return Err(shellcheck_fail("workflow_mapping_property_malformed"));
    }
    let name = &rest[..name_len];
    let trailing = rest[name_len..].trim();
    match property {
        b'&' if trailing.is_empty() || trailing == "{}" => Ok(Some(MappingProperty::Anchor {
            name: name.to_owned(),
            inline_empty: trailing == "{}",
        })),
        b'*' if trailing.is_empty() => Ok(Some(MappingProperty::Alias(name.to_owned()))),
        _ => Err(shellcheck_fail("workflow_mapping_property_malformed")),
    }
}

pub(super) fn parse_step_property(
    content: &str,
) -> Result<Option<StepProperty>, OrchestratorError> {
    let Some(value) = content.strip_prefix("- ") else {
        return Ok(None);
    };
    let value = value.trim();
    let Some(property) = value.as_bytes().first().copied() else {
        return Ok(None);
    };
    if !matches!(property, b'&' | b'*') {
        return Ok(None);
    }
    let rest = &value[1..];
    let name_len = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        .count();
    if name_len == 0 || !rest[name_len..].trim().is_empty() {
        return Err(shellcheck_fail("workflow_step_property_malformed"));
    }
    let name = rest[..name_len].to_owned();
    match property {
        b'&' => Ok(Some(StepProperty::Anchor(name))),
        b'*' => Ok(Some(StepProperty::Alias(name))),
        _ => unreachable!(),
    }
}

pub(super) fn reject_non_mapping_content(content: &str) -> Result<(), OrchestratorError> {
    let content = content.strip_prefix("- ").unwrap_or(content).trim_start();
    if has_node_property(content) {
        return Err(shellcheck_fail("workflow_alias_outside_step_run"));
    }
    if flow_value_after_tags(content).is_some_and(flow_contains_merge_key) {
        return Err(shellcheck_fail("workflow_merge_key_unsupported"));
    }
    Ok(())
}

fn has_node_property(value: &str) -> bool {
    let mut value = value.trim_start();
    loop {
        if value.starts_with(['&', '*']) {
            return true;
        }
        if !starts_with_tag_property(value) {
            break;
        }
        value = skip_tag_property(value).trim_start();
    }
    flow_value_after_tags(value).is_some_and(flow_contains_node_property)
}

fn starts_with_tag_property(value: &str) -> bool {
    value.trim_start().starts_with('!')
}

fn flow_value_after_tags(value: &str) -> Option<&str> {
    let mut value = value.trim_start();
    while starts_with_tag_property(value) {
        value = skip_tag_property(value).trim_start();
    }
    value.starts_with(['[', '{']).then_some(value)
}

fn flow_contains_node_property(value: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_usize;
    let mut node_start = true;
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
            '"' | '\'' => {
                quote = Some(character);
                node_start = false;
            }
            '[' | '{' => {
                depth = depth.saturating_add(1);
                node_start = true;
            }
            ']' | '}' => {
                depth = depth.saturating_sub(1);
                node_start = false;
            }
            ',' | ':' if depth > 0 => node_start = true,
            '!' if node_start => consume_tag_property(&mut characters),
            '&' | '*' if depth > 0 && node_start => return true,
            character if character.is_whitespace() => {}
            _ => node_start = false,
        }
        previous_character = Some(character);
    }
    false
}

fn flow_contains_merge_key(value: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_usize;
    let mut node_start = true;
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
        if character == '<' && node_start && characters.peek() == Some(&'<') {
            let mut lookahead = characters.clone();
            let _ = lookahead.next();
            while lookahead.peek().is_some_and(|next| next.is_whitespace()) {
                let _ = lookahead.next();
            }
            if lookahead.peek() == Some(&':') {
                return true;
            }
        }
        match character {
            '"' | '\'' => {
                quote = Some(character);
                node_start = false;
            }
            '[' | '{' => {
                depth = depth.saturating_add(1);
                node_start = true;
            }
            ']' | '}' => {
                depth = depth.saturating_sub(1);
                node_start = false;
            }
            ',' | ':' if depth > 0 => node_start = true,
            '!' if node_start => consume_tag_property(&mut characters),
            character if character.is_whitespace() => {}
            _ => node_start = false,
        }
        previous_character = Some(character);
    }
    false
}

fn skip_tag_property(value: &str) -> &str {
    let Some(after_bang) = value.strip_prefix('!') else {
        return value;
    };
    if after_bang.starts_with('<') {
        return after_bang
            .find('>')
            .map_or("", |end| &after_bang[end + 1..]);
    }
    let end = after_bang
        .char_indices()
        .find(|(_, character)| {
            character.is_whitespace() || matches!(character, '[' | ']' | '{' | '}' | ',')
        })
        .map_or(after_bang.len(), |(index, _)| index);
    &after_bang[end..]
}

fn consume_tag_property(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    if characters.peek() == Some(&'<') {
        for character in characters.by_ref() {
            if character == '>' {
                break;
            }
        }
        return;
    }
    while characters.peek().is_some_and(|character| {
        !character.is_whitespace() && !matches!(character, '[' | ']' | '{' | '}' | ',')
    }) {
        let _ = characters.next();
    }
}
