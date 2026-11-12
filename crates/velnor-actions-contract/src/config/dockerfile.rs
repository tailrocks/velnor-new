// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DockerfileFacts {
    pub external_images: BTreeSet<String>,

    pub context_sources: BTreeSet<String>,

    pub named_contexts: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerfileProblem {
    ImageBaseUnpinned(String),

    ImageBaseUnresolved(String),

    SecretMount,

    RemoteAdd(String),

    ContextEscape(String),

    EscapeDirective,

    SyntaxUnpinned(String),

    UnsupportedRunnerSyntax(String),
}

impl DockerfileProblem {
    #[must_use]
    pub fn code(&self) -> String {
        match self {
            Self::ImageBaseUnpinned(image) => format!("image_base_unpinned:{image}"),
            Self::ImageBaseUnresolved(image) => format!("image_base_unresolved:{image}"),
            Self::SecretMount => "image_secret_mount".to_owned(),
            Self::RemoteAdd(source) => format!("image_add_remote_source:{source}"),
            Self::ContextEscape(source) => format!("image_context_escape:{source}"),
            Self::EscapeDirective => "image_escape_directive".to_owned(),
            Self::SyntaxUnpinned(image) => format!("image_syntax_unpinned:{image}"),
            Self::UnsupportedRunnerSyntax(value) => {
                format!("runner_dockerfile_unsupported:{value}")
            }
        }
    }
}

mod runner;
mod syntax;

pub use runner::analyze_runner_dockerfile;
use syntax::{check_directives, instructions};

#[must_use]
pub fn is_digest_pinned(reference: &str) -> bool {
    reference
        .rsplit_once("@sha256:")
        .is_some_and(|(name, hex)| {
            !name.is_empty()
                && hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

pub fn analyze_dockerfile(
    text: &str,
    known_contexts: &[&str],
) -> Result<DockerfileFacts, DockerfileProblem> {
    let mut state = State {
        known_contexts: known_contexts
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        ..State::default()
    };
    check_directives(text)?;
    for (keyword, args) in instructions(text) {
        match keyword.as_str() {
            "ARG" if !state.in_stage => state.record_global_arg(&args),
            "FROM" => state.from(&args)?,
            "COPY" | "ADD" => state.copy(&keyword, &args)?,
            "RUN" => state.run(&args)?,
            _ => {}
        }
    }
    Ok(state.facts)
}

#[derive(Default)]
struct State {
    facts: DockerfileFacts,
    in_stage: bool,
    args: BTreeMap<String, Option<String>>,
    stages: BTreeSet<String>,
    stage_count: usize,
    known_contexts: BTreeSet<String>,
}

impl State {
    fn record_global_arg(&mut self, args: &str) {
        for token in args.split_whitespace() {
            match token.split_once('=') {
                Some((name, value)) => {
                    self.args
                        .insert(name.to_owned(), Some(unquote(value).to_owned()));
                }
                None => {
                    self.args.insert(token.to_owned(), None);
                }
            }
        }
    }

    fn from(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        self.in_stage = true;
        let mut tokens = args.split_whitespace().filter(|t| !t.starts_with("--"));
        let Some(raw) = tokens.next() else {
            return Ok(());
        };
        let reference = self.substitute(raw)?;
        let is_stage = self.stages.contains(&reference.to_ascii_lowercase());
        if reference != "scratch" && !is_stage {
            require_pinned(&reference)?;
            self.facts.external_images.insert(reference);
        }
        if tokens.next().is_some_and(|t| t.eq_ignore_ascii_case("AS"))
            && let Some(name) = tokens.next()
        {
            self.stages.insert(name.to_ascii_lowercase());
        }
        self.stage_count += 1;
        Ok(())
    }

    fn substitute(&self, raw: &str) -> Result<String, DockerfileProblem> {
        let mut out = String::new();
        let mut rest = raw;
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            let tail = &rest[at + 1..];
            let (name, default, consumed) = parse_variable(tail);
            let value = match self.args.get(name) {
                Some(Some(value)) => value.clone(),
                _ => default
                    .map(str::to_owned)
                    .ok_or_else(|| DockerfileProblem::ImageBaseUnresolved(raw.to_owned()))?,
            };
            out.push_str(&value);
            rest = &tail[consumed..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn copy(&mut self, keyword: &str, args: &str) -> Result<(), DockerfileProblem> {
        let mut from_other = false;
        let mut operands = Vec::new();
        for token in split_operands(args) {
            if let Some(source) = token.strip_prefix("--from=") {
                from_other = true;
                self.check_from_source(source)?;
            } else if !token.starts_with("--") {
                operands.push(token);
            }
        }
        if from_other || operands.len() < 2 {
            return Ok(());
        }
        operands.pop();
        for source in operands {
            if source.starts_with("<<") {
                continue;
            }
            if keyword == "ADD" && is_remote(&source) {
                return Err(DockerfileProblem::RemoteAdd(source));
            }
            let relative = source.trim_start_matches("./").trim_start_matches('/');
            if relative.split('/').any(|part| part == "..") {
                return Err(DockerfileProblem::ContextEscape(source));
            }
            self.facts.context_sources.insert(relative.to_owned());
        }
        Ok(())
    }

    fn check_from_source(&mut self, source: &str) -> Result<(), DockerfileProblem> {
        let is_stage = source.bytes().all(|b| b.is_ascii_digit())
            || self.stages.contains(&source.to_ascii_lowercase());
        if self.known_contexts.contains(source) {
            self.facts.named_contexts.insert(source.to_owned());
        } else if !is_stage {
            require_pinned(source)?;
            self.facts.external_images.insert(source.to_owned());
        }
        Ok(())
    }

    fn run(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        for token in args.split_whitespace().take_while(|t| t.starts_with("--")) {
            let Some(mount) = token.strip_prefix("--mount=") else {
                continue;
            };
            for field in mount.split(',') {
                match field.split_once('=') {
                    Some(("type", "secret" | "ssh")) => return Err(DockerfileProblem::SecretMount),
                    Some(("from", source)) => self.check_from_source(source)?,
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn require_pinned(reference: &str) -> Result<(), DockerfileProblem> {
    if is_digest_pinned(reference) {
        Ok(())
    } else {
        Err(DockerfileProblem::ImageBaseUnpinned(reference.to_owned()))
    }
}

fn is_remote(source: &str) -> bool {
    source.contains("://") || source.starts_with("git@")
}

fn unquote(value: &str) -> &str {
    value.trim_matches(|c| c == '"' || c == '\'')
}

fn parse_variable(tail: &str) -> (&str, Option<&str>, usize) {
    if let Some(body) = tail.strip_prefix('{')
        && let Some(end) = body.find('}')
    {
        let inner = &body[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (inner, None),
        };
        return (name, default, end + 2);
    }
    let end = tail
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() || b == b'_'))
        .unwrap_or(tail.len());
    (&tail[..end], None, end)
}

fn split_operands(args: &str) -> Vec<String> {
    let trimmed = args.trim();
    if trimmed.starts_with('[')
        && let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
    {
        return inner
            .split(',')
            .map(|item| unquote(item.trim()).to_owned())
            .collect();
    }
    trimmed
        .split_whitespace()
        .map(|item| unquote(item).to_owned())
        .collect()
}
