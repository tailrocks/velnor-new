// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{DockerfileProblem, is_digest_pinned};
use std::iter::Peekable;
use std::str::Chars;

pub(super) fn check_directives(text: &str) -> Result<bool, DockerfileProblem> {
    let mut custom_frontend = false;
    for line in text.lines() {
        let Some(directive) = line.trim().strip_prefix('#') else {
            break;
        };
        let Some((key, value)) = directive.split_once('=') else {
            break;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "escape" => return Err(DockerfileProblem::EscapeDirective),
            "syntax" if !is_digest_pinned(value.trim()) => {
                return Err(DockerfileProblem::SyntaxUnpinned(value.trim().to_owned()));
            }
            "syntax" => {
                let name = value.trim().split('@').next().unwrap_or_default();
                let name = name.split(':').next().unwrap_or_default();
                custom_frontend |=
                    !matches!(name, "docker/dockerfile" | "docker.io/docker/dockerfile");
            }
            _ => {}
        }
    }
    Ok(custom_frontend)
}

pub(super) fn instructions(text: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let mut logical = lines[index].trim().to_owned();
        index += 1;
        if logical.is_empty() || logical.starts_with('#') {
            continue;
        }
        while logical.ends_with('\\') {
            logical.pop();
            let Some(next) = lines.get(index) else { break };
            index += 1;
            if !next.trim().starts_with('#') {
                logical.push(' ');
                logical.push_str(next.trim());
            }
        }
        let (keyword, args) = logical
            .split_once(char::is_whitespace)
            .unwrap_or((logical.as_str(), ""));
        let keyword = keyword.to_ascii_uppercase();
        if takes_heredoc(&keyword, args) {
            index = heredoc_end(&lines, index, &logical);
        }
        out.push((keyword, args.trim().to_owned()));
    }
    out
}

fn heredoc_end(lines: &[&str], start: usize, logical: &str) -> usize {
    let mut index = start;
    for name in heredoc_names(logical) {
        let Some(offset) = lines[index..]
            .iter()
            .position(|line| line.trim() == name.as_str())
        else {
            return start;
        };
        index += offset + 1;
    }
    index
}

fn takes_heredoc(keyword: &str, args: &str) -> bool {
    let (child, child_args) = if keyword == "ONBUILD" {
        let Some((child, child_args)) = args.split_once(char::is_whitespace) else {
            return false;
        };
        (child, child_args)
    } else {
        (keyword, args)
    };
    let child = child.to_ascii_uppercase();
    matches!(child.as_str(), "ADD" | "COPY" | "RUN") && !child_args.trim_start().starts_with('[')
}

fn heredoc_names(logical: &str) -> Vec<String> {
    let Some(words) = shell_words(logical, true) else {
        return Vec::new();
    };
    words.iter().filter_map(|word| heredoc_name(word)).collect()
}

fn heredoc_name(word: &str) -> Option<String> {
    let rest = word.trim_start_matches(|character: char| character.is_ascii_digit());
    let rest = rest.strip_prefix("<<")?;
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    let rest = rest.trim_start_matches([' ', '\t', '\n', '\u{c}', '\r']);
    if rest.is_empty() || rest.contains('<') {
        return None;
    }
    let mut names = shell_words(rest, false)?;
    if names.len() != 1 {
        return None;
    }
    Some(names.swap_remove(0))
}

fn shell_words(line: &str, raw: bool) -> Option<Vec<String>> {
    if malformed_substitution(line) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\'' => {
                let quoted = single_quoted(&mut chars)?;
                push_quoted(&mut word, &quoted, raw, '\'');
                started = true;
            }
            '"' => {
                let quoted = double_quoted(&mut chars, raw)?;
                push_quoted(&mut word, &quoted, raw, '"');
                started = true;
            }
            '\\' => {
                let Some(escaped) = chars.next() else { break };
                if raw {
                    word.push('\\');
                }
                word.push(escaped);
                started = true;
            }
            '$' => {
                word.push('$');
                started = true;
            }
            '<' => {
                push_shift(&mut word, &mut chars);
                started = true;
            }
            character if character.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            character => {
                word.push(character);
                started = true;
            }
        }
    }
    if started {
        out.push(word);
    }
    Some(out)
}

fn push_shift(word: &mut String, chars: &mut Peekable<Chars<'_>>) {
    word.push('<');
    if chars.peek() != Some(&'<') {
        return;
    }
    chars.next();
    word.push('<');
    while matches!(chars.peek(), Some(' ' | '\t' | '\r')) {
        let Some(spaced) = chars.next() else { break };
        word.push(spaced);
    }
}

fn single_quoted(chars: &mut Peekable<Chars<'_>>) -> Option<String> {
    let mut text = String::new();
    for character in chars.by_ref() {
        if character == '\'' {
            return Some(text);
        }
        text.push(character);
    }
    None
}

fn double_quoted(chars: &mut Peekable<Chars<'_>>, raw: bool) -> Option<String> {
    let mut text = String::new();
    while let Some(character) = chars.next() {
        match character {
            '"' => return Some(text),
            '\\' => match chars.peek() {
                None => return None,
                Some('"' | '$' | '\\') => {
                    let escaped = chars.next()?;
                    if raw {
                        text.push('\\');
                    }
                    text.push(escaped);
                }
                Some(_) => {
                    if raw {
                        text.push('\\');
                    }
                }
            },
            _ => text.push(character),
        }
    }
    None
}

fn push_quoted(word: &mut String, quoted: &str, raw: bool, quote: char) {
    if raw {
        word.push(quote);
        word.push_str(quoted);
        word.push(quote);
    } else {
        word.push_str(quoted);
    }
}

fn malformed_substitution(line: &str) -> bool {
    let mut rest = line;
    while let Some(at) = rest.find("${") {
        let tail = &rest[at + 2..];
        match tail.chars().next() {
            Some('{' | '}' | ':') | None => return true,
            Some(_) => {}
        }
        let Some(close) = tail.find('}') else {
            return true;
        };
        rest = &tail[close + 1..];
    }
    false
}
