// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) const MAX_EXPRESSION_CHARACTERS: usize = 21_000;

fn escaped(literal: &str) -> usize {
    literal
        .chars()
        .map(|character| if "'{}".contains(character) { 2 } else { 1 })
        .sum()
}

pub(crate) fn joined_length(text: &str) -> usize {
    let mut rest = text;
    let mut literal = 0;
    let mut placeholders = 0;
    let mut expressions = Vec::new();
    while let Some((before, after)) = rest.split_once("${{") {
        let Some((expression, tail)) = after.split_once("}}") else {
            break;
        };
        literal += escaped(before);
        placeholders += 2 + expressions.len().to_string().len();
        expressions.push(expression.trim().len());
        rest = tail;
    }
    literal += escaped(rest);
    match expressions.as_slice() {
        [] => 0,
        [only] if literal == 0 => *only,
        _ => {
            "format('".len()
                + literal
                + placeholders
                + "'".len()
                + expressions.iter().map(|length| 2 + length).sum::<usize>()
                + ")".len()
        }
    }
}
