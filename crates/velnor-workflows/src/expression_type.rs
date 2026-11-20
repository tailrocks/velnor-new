// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::plan::InputKind;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResultType {
    Text,
    Boolean,
}

impl ResultType {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Boolean => "a boolean",
        }
    }

    pub(crate) fn fits(self, kind: Option<&InputKind>) -> bool {
        !matches!(
            (self, kind),
            (Self::Text, Some(InputKind::Boolean | InputKind::Number))
                | (Self::Boolean, Some(InputKind::Number))
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Operator {
    None,
    Negation,
    Comparison,
    Logical,
}

struct TopLevel {
    operator: Operator,
    call: Option<String>,
    quoted_whole: bool,
}

fn top_level(expression: &str) -> TopLevel {
    let characters: Vec<char> = expression.chars().collect();
    let mut found = TopLevel {
        operator: if characters.first() == Some(&'!') && characters.get(1) != Some(&'=') {
            Operator::Negation
        } else {
            Operator::None
        },
        call: None,
        quoted_whole: false,
    };
    let mut depth = 0usize;
    let mut quoted = false;
    let mut closed_at = None;
    for (index, &c) in characters.iter().enumerate() {
        let next = characters.get(index + 1).copied();
        if c == '\'' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && c == ')' && closed_at.is_none() {
                    closed_at = Some(index);
                }
            }
            '&' | '|' if depth == 0 && next == Some(c) => found.operator = Operator::Logical,
            '=' | '!' if depth == 0 && next == Some('=') => {
                found.operator = found.operator.max(Operator::Comparison);
            }
            '<' | '>' if depth == 0 => found.operator = found.operator.max(Operator::Comparison),
            _ => {}
        }
    }
    if closed_at == Some(characters.len().saturating_sub(1))
        && let Some(open) = expression.find('(')
    {
        found.call = Some(expression[..open].trim().to_owned());
    }
    found.quoted_whole = expression.len() > 1
        && expression.starts_with('\'')
        && expression.ends_with('\'')
        && !expression[1..expression.len() - 1]
            .replace("''", "")
            .contains('\'');
    found
}

fn single_reference(expression: &str) -> bool {
    !expression.is_empty()
        && expression
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.*-".contains(c))
}

/// The type an expression certainly evaluates to, or `None` when it could be any.
pub(crate) fn result_type(expression: &str) -> Option<ResultType> {
    let expression = expression.trim();
    let found = top_level(expression);
    match found.operator {
        Operator::Logical => return None,
        Operator::Comparison | Operator::Negation => return Some(ResultType::Boolean),
        Operator::None => {}
    }
    if found.quoted_whole {
        return Some(ResultType::Text);
    }
    if let Some(function) = found.call {
        return match function.as_str() {
            "contains" | "startsWith" | "endsWith" => Some(ResultType::Boolean),
            "format" | "toJSON" | "join" | "hashFiles" => Some(ResultType::Text),
            _ => None,
        };
    }
    if !single_reference(expression) {
        return None;
    }
    let parts: Vec<&str> = expression.split('.').collect();
    match parts.as_slice() {
        ["true" | "false"] => Some(ResultType::Boolean),
        ["needs" | "steps", _, "outputs", _] => Some(ResultType::Text),
        ["github", name] if *name != "event" => Some(ResultType::Text),
        _ => None,
    }
}
