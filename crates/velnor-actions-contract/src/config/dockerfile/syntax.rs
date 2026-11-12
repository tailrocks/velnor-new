// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{DockerfileProblem, is_digest_pinned};

pub(super) fn check_directives(text: &str) -> Result<(), DockerfileProblem> {
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
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn instructions(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let mut logical = line.trim().to_owned();
        if logical.is_empty() || logical.starts_with('#') {
            continue;
        }
        while logical.ends_with('\\') {
            logical.pop();
            let Some(next) = lines.next() else { break };
            if !next.trim().starts_with('#') {
                logical.push(' ');
                logical.push_str(next.trim());
            }
        }
        for word in heredoc_words(&logical) {
            for body in lines.by_ref() {
                if body.trim() == word {
                    break;
                }
            }
        }
        let (keyword, args) = logical
            .split_once(char::is_whitespace)
            .unwrap_or((logical.as_str(), ""));
        out.push((keyword.to_ascii_uppercase(), args.trim().to_owned()));
    }
    out
}

fn heredoc_words(logical: &str) -> Vec<String> {
    logical
        .split_whitespace()
        .filter_map(|token| token.strip_prefix("<<"))
        .map(|word| {
            word.trim_start_matches('-')
                .trim_matches(['"', '\''])
                .to_owned()
        })
        .filter(|word| !word.is_empty())
        .collect()
}
