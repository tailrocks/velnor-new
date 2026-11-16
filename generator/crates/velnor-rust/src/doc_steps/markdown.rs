// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const TEST_WORDS: [&str; 9] = [
    "rust",
    ".rust",
    "should_panic",
    "no_run",
    "compile_fail",
    "ignore",
    "test_harness",
    "standalone_crate",
    "fail",
];

/// The lines of a block doc as Markdown sees them: the opening line, then the
/// rest without a leading `*` column and without their common indentation.
/// When the opening line carries text, the later lines keep all their
/// indentation, so an indented example there stays a code block.
pub(crate) fn block_lines(body: &str) -> Vec<&str> {
    let body = body.strip_suffix("*/").unwrap_or(body);
    let mut lines = body.lines();
    let first = lines.next().unwrap_or("");
    let rest: Vec<&str> = lines.collect();
    let starred = rest
        .iter()
        .filter(|line| !line.trim().is_empty())
        .all(|line| line.trim_start().starts_with('*'));
    let rest: Vec<&str> = rest
        .into_iter()
        .map(|line| {
            if starred {
                line.trim_start().strip_prefix('*').unwrap_or(line)
            } else {
                line
            }
        })
        .collect();
    let indent = if first.trim().is_empty() {
        rest.iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
            .min()
            .unwrap_or(0)
    } else {
        0
    };
    std::iter::once(first)
        .chain(
            rest.into_iter()
                .map(|line| line.get(indent..).unwrap_or("")),
        )
        .collect()
}

/// Whether Markdown text holds a code block rustdoc compiles as a test: an
/// indented block, or a fence with no language or only Rust and rustdoc words.
pub(crate) fn tested<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    let mut open: Option<(char, usize)> = None;
    for line in lines {
        if let Some((mark, length)) = open {
            if closes(line, mark, length) {
                open = None;
            }
        } else if let Some((mark, length, info)) = opens(line) {
            if rust_info(info) {
                return true;
            }
            open = Some((mark, length));
        } else if !line.trim().is_empty() && indented_code(line) {
            return true;
        }
    }
    false
}

pub(crate) fn tested_text(text: &str) -> bool {
    tested(text.lines())
}

pub(crate) fn code_block(line: &str) -> bool {
    line.contains("```") || line.contains("~~~") || indented_code(line)
}

fn quoted(line: &str) -> &str {
    line.trim_start_matches([' ', '\t', '>'])
}

fn run_of(text: &str) -> Option<(char, usize)> {
    let mark = text.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let length = text.chars().take_while(|&c| c == mark).count();
    (length >= 3).then_some((mark, length))
}

fn opens(line: &str) -> Option<(char, usize, &str)> {
    let mut text = quoted(line);
    while let Some(after) = list_marker(text) {
        text = quoted(after);
    }
    let (mark, length) = run_of(text)?;
    let info = &text[length..];
    (mark != '`' || !info.contains('`')).then_some((mark, length, info))
}

fn closes(line: &str, mark: char, length: usize) -> bool {
    let text = quoted(line);
    run_of(text)
        .is_some_and(|(found, run)| found == mark && run >= length && text[run..].trim().is_empty())
}

fn rust_info(info: &str) -> bool {
    info.split([',', ' ', '\t'])
        .filter(|word| !word.is_empty())
        .all(|word| {
            TEST_WORDS.contains(&word)
                || word.starts_with("edition")
                || word.starts_with("ignore-")
                || word
                    .strip_prefix('E')
                    .is_some_and(|code| code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit()))
        })
}

fn indented_code(rest: &str) -> bool {
    let mut text = rest;
    loop {
        let trimmed = text.trim_start_matches([' ', '\t']);
        let indent = &text[..text.len() - trimmed.len()];
        if indent.contains('\t') || indent.len() >= 4 {
            return true;
        }
        match trimmed.strip_prefix('>').or_else(|| list_marker(trimmed)) {
            Some(after) => text = after,
            None => return false,
        }
    }
}

fn list_marker(text: &str) -> Option<&str> {
    let after = text.strip_prefix(['*', '-', '+']).or_else(|| {
        let digits = text.trim_start_matches(|c: char| c.is_ascii_digit());
        if digits.len() < text.len() {
            digits.strip_prefix(['.', ')'])
        } else {
            None
        }
    })?;
    after.starts_with([' ', '\t']).then_some(after)
}
