// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    DockerfileFacts, DockerfileProblem, analyze_dockerfile, instructions, is_digest_pinned,
};
use std::collections::BTreeSet;

pub fn analyze_runner_dockerfile(
    text: &str,
    known_contexts: &[&str],
) -> Result<DockerfileFacts, DockerfileProblem> {
    validate_runner_syntax(text, known_contexts)?;
    analyze_dockerfile(text, known_contexts)
}

fn runner_unsupported(value: &str) -> DockerfileProblem {
    DockerfileProblem::UnsupportedRunnerSyntax(value.to_owned())
}

fn validate_runner_syntax(text: &str, known_contexts: &[&str]) -> Result<(), DockerfileProblem> {
    for line in text.lines() {
        let directive = line.trim().strip_prefix('#').map(str::trim);
        if directive
            .and_then(|line| line.split_once('='))
            .is_some_and(|(key, _)| {
                matches!(
                    key.trim().to_ascii_lowercase().as_str(),
                    "syntax" | "escape" | "check"
                )
            })
        {
            return Err(runner_unsupported("directive"));
        }
    }
    let mut aliases = BTreeSet::new();
    let mut base = false;
    for (keyword, args) in instructions(text) {
        match keyword.as_str() {
            "FROM" => {
                let words: Vec<_> = args.split_whitespace().collect();
                match words.as_slice() {
                    ["scratch", alias, name]
                        if alias.eq_ignore_ascii_case("as")
                            && !base
                            && known_contexts.contains(name)
                            && aliases.insert((*name).to_owned()) => {}
                    [image]
                        if !base
                            && !image.contains('$')
                            && (*image == "scratch" || is_digest_pinned(image)) =>
                    {
                        base = true;
                    }
                    _ => return Err(runner_unsupported("from")),
                }
            }
            "COPY" => {
                if !base {
                    return Err(runner_unsupported("copy_stage"));
                }
                let words: Vec<_> = args.split_whitespace().collect();
                let operands = match words.as_slice() {
                    [source, destination] if !source.starts_with('-') => [*source, *destination],
                    [from, source, destination]
                        if from
                            .strip_prefix("--from=")
                            .is_some_and(|name| known_contexts.contains(&name)) =>
                    {
                        [*source, *destination]
                    }
                    _ => return Err(runner_unsupported("copy")),
                };
                if operands.iter().any(|part| {
                    part.is_empty()
                        || !part.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'/' | b'.' | b'_' | b'-')
                        })
                        || part.split('/').any(|part| part == "..")
                }) {
                    return Err(runner_unsupported("copy_path"));
                }
            }
            "RUN" if !base || args.starts_with("--") || args.contains("<<") => {
                return Err(runner_unsupported("run"));
            }
            "ARG" | "RUN" | "ENTRYPOINT" | "LABEL" | "USER" | "WORKDIR" => {}
            _ => return Err(runner_unsupported("instruction")),
        }
    }
    if !base {
        return Err(runner_unsupported("from"));
    }
    Ok(())
}
