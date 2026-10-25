//! Quote-aware expression substitution for the private reusable-workflow prototype.

use super::{STEP_REFERENCE_BINDINGS, invalid};
use crate::RenderError;

pub(super) fn rewrite_condition(
    job_id: &str,
    field: &str,
    value: &mut String,
) -> Result<(), RenderError> {
    if value.contains("${{") {
        *value = rewrite_expressions(value, job_id, field)?;
    } else {
        *value = rewrite_expression_body(value, job_id, field)?;
    }
    Ok(())
}

pub(super) fn rewrite_expressions(
    value: &str,
    job_id: &str,
    field: &str,
) -> Result<String, RenderError> {
    let mut output = String::with_capacity(value.len());
    let mut copied = 0;
    while let Some(relative_start) = value[copied..].find("${{") {
        let start = copied + relative_start;
        let expression_start = start + 3;
        let Some(end) = expression_end(value, expression_start) else {
            return Err(invalid(job_id, &format!("unterminated_expression:{field}")));
        };
        output.push_str(&value[copied..expression_start]);
        output.push_str(&rewrite_expression_body(
            &value[expression_start..end],
            job_id,
            field,
        )?);
        output.push_str("}}");
        copied = end + 2;
    }
    output.push_str(&value[copied..]);
    Ok(output)
}

/// Quote-aware closing-delimiter scan following the contract cache-expression rewriter.
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
    expression: &str,
    job_id: &str,
    field: &str,
) -> Result<String, RenderError> {
    let mut output = String::with_capacity(expression.len());
    let mut copied = 0;
    let mut index = 0;
    let mut quote = None;
    while index < expression.len() {
        let Some(character) = expression[index..].chars().next() else {
            return Err(invalid(
                job_id,
                &format!("invalid_expression_index:{field}"),
            ));
        };
        if let Some(active_quote) = quote {
            if character == active_quote {
                let doubled_single_quote = active_quote == '\''
                    && expression[index + character.len_utf8()..].starts_with('\'');
                if doubled_single_quote {
                    index += character.len_utf8() + 1;
                    continue;
                }
                quote = None;
            } else if active_quote == '"' && character == '\\' {
                index += character.len_utf8();
                if let Some(escaped_character) = expression[index..].chars().next() {
                    index += escaped_character.len_utf8();
                }
                continue;
            }
            index += character.len_utf8();
            continue;
        }
        if character == '\'' || character == '"' {
            quote = Some(character);
            index += character.len_utf8();
            continue;
        }
        if is_needs_identifier(expression, index) {
            let (replacement, end) = rewrite_needs_reference(expression, index)
                .ok_or_else(|| invalid(job_id, &format!("unbound_needs_reference:{field}")))?;
            output.push_str(&expression[copied..index]);
            output.push_str(replacement);
            index = end;
            copied = end;
            continue;
        }
        index += character.len_utf8();
    }
    if quote.is_some() {
        return Err(invalid(job_id, &format!("unterminated_literal:{field}")));
    }
    output.push_str(&expression[copied..]);
    Ok(output)
}

fn is_needs_identifier(expression: &str, index: usize) -> bool {
    let Some(after) = expression.get(index..index + "needs".len()) else {
        return false;
    };
    if !after.eq_ignore_ascii_case("needs") {
        return false;
    }
    let after = &expression[index + "needs".len()..];
    let before_is_identifier = expression[..index]
        .chars()
        .next_back()
        .is_some_and(is_identifier_character);
    let after_is_identifier = after.chars().next().is_some_and(is_identifier_character);
    !before_is_identifier && !after_is_identifier
}

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '-'
}

fn rewrite_needs_reference(expression: &str, index: usize) -> Option<(&'static str, usize)> {
    let fallback = "needs.plan.outputs.plan_attempt";
    if expression[index..].starts_with(fallback) {
        let whitespace_end = skip_whitespace(expression, index + fallback.len());
        if expression[whitespace_end..].starts_with("||") {
            let right_start = skip_whitespace(expression, whitespace_end + 2);
            let right_end = right_start + "github.run_attempt".len();
            if expression.get(right_start..right_end) == Some("github.run_attempt")
                && has_reference_boundary(expression, right_end)
                && expression[..index].trim().is_empty()
                && expression[right_end..].trim().is_empty()
            {
                return Some(("inputs.run_key_attempt", right_end));
            }
        }
        // The plan attempt is only transported as the complete caller-computed fallback.
    }
    STEP_REFERENCE_BINDINGS
        .iter()
        .find_map(|(source, replacement)| {
            let end = index + source.len();
            (expression[index..].starts_with(source) && has_reference_boundary(expression, end))
                .then_some((*replacement, end))
        })
}

fn skip_whitespace(value: &str, mut index: usize) -> usize {
    while let Some(character) = value[index..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        index += character.len_utf8();
    }
    index
}

fn has_reference_boundary(expression: &str, index: usize) -> bool {
    expression[index..]
        .chars()
        .next()
        .is_none_or(|next| !is_identifier_character(next) && next != '.' && next != '[')
}
