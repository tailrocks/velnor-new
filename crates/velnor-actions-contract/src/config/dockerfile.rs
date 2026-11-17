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

    ContextSourceUnresolved(String),

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
            Self::ContextSourceUnresolved(source) => {
                format!("image_context_source_unresolved:{source}")
            }
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
mod variable;

pub use runner::analyze_runner_dockerfile;
use syntax::{check_directives, instructions};
use variable::{arg_value, is_injected_arg, substitute};

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
            "ARG" => state.record_arg(&args),
            "FROM" => state.from(&args)?,
            "ENV" => state.unknown_environment = true,
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
    unknown_environment: bool,
    inherited_stage: bool,
    global_args: BTreeMap<String, Option<String>>,
    stage_args: BTreeMap<String, Option<String>>,
    stages: BTreeSet<String>,
    stage_count: usize,
    known_contexts: BTreeSet<String>,
}

impl State {
    fn record_arg(&mut self, args: &str) {
        let token = args.trim();
        let (name, value) = match token.split_once('=') {
            Some((name, value)) => (name, arg_value(value)),
            None if self.inherited_stage => (token, None),
            None => (
                token,
                self.stage_args
                    .get(token)
                    .or_else(|| self.global_args.get(token))
                    .cloned()
                    .flatten(),
            ),
        };
        let value = if is_injected_arg(name) { None } else { value };
        if self.in_stage {
            self.stage_args.insert(name.to_owned(), value);
        } else {
            self.global_args.insert(name.to_owned(), value);
        }
    }

    fn from(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        let mut tokens = args.split_whitespace().filter(|t| !t.starts_with("--"));
        let Some(raw) = tokens.next() else {
            return Ok(());
        };
        let reference = substitute(raw, &self.global_args)
            .ok_or_else(|| DockerfileProblem::ImageBaseUnresolved(raw.to_owned()))?;
        self.in_stage = true;
        self.stage_args.clear();
        let is_stage = self.stages.contains(&reference.to_ascii_lowercase());
        self.inherited_stage = is_stage
            || reference
                .parse::<usize>()
                .is_ok_and(|index| index < self.stage_count);
        self.unknown_environment = reference != "scratch";
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

    fn copy(&mut self, keyword: &str, args: &str) -> Result<(), DockerfileProblem> {
        if args.contains('\\')
            || (!args.trim_start().starts_with('[') && args.contains(['"', '\'']))
        {
            return Err(DockerfileProblem::ContextSourceUnresolved(args.to_owned()));
        }
        let mut from_other = false;
        let mut operands = Vec::new();
        for token in split_operands(args)
            .ok_or_else(|| DockerfileProblem::ContextSourceUnresolved(args.to_owned()))?
        {
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
            if source.contains(['\\', '\'', '"'])
                || (self.unknown_environment && source.contains('$'))
            {
                return Err(DockerfileProblem::ContextSourceUnresolved(source));
            }
            let source = substitute(&source, &self.stage_args)
                .ok_or_else(|| DockerfileProblem::ContextSourceUnresolved(source.clone()))?;
            if !args.trim_start().starts_with('[') && source.chars().any(char::is_whitespace) {
                return Err(DockerfileProblem::ContextSourceUnresolved(source));
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

fn split_operands(args: &str) -> Option<Vec<String>> {
    let trimmed = args.trim();
    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).ok();
    }
    Some(
        trimmed
            .split_whitespace()
            .map(|item| unquote(item).to_owned())
            .collect(),
    )
}
