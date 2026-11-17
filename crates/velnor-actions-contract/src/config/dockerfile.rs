// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DockerfileFacts {
    pub external_images: BTreeSet<String>,

    pub context_sources: BTreeSet<String>,

    pub claims_whole_context: bool,

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
use variable::{ArgValue, arg_value, arg_words, is_injected_arg, substitute, valid_name};

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
    state.facts.claims_whole_context = check_directives(text)?;
    for (keyword, args) in instructions(text) {
        state.instruction(&keyword, &args)?;
    }
    Ok(state.facts)
}

#[derive(Default)]
struct State {
    facts: DockerfileFacts,
    unknown_environment: bool,
    inherited_stage: bool,
    global_args: BTreeMap<String, ArgValue>,
    stage_args: BTreeMap<String, ArgValue>,
    stages: BTreeMap<String, usize>,
    triggers: Vec<Vec<(String, String)>>,
    known_contexts: BTreeSet<String>,
}

impl State {
    fn instruction(&mut self, keyword: &str, args: &str) -> Result<(), DockerfileProblem> {
        match keyword {
            "ARG" => self.record_arg(args)?,
            "FROM" => self.from(args)?,
            "ENV" => self.unknown_environment = true,
            "COPY" | "ADD" => self.copy(keyword, args)?,
            "RUN" => self.run(args)?,
            "ONBUILD" => {
                let parsed = instructions(args);
                if parsed.is_empty() {
                    self.facts.claims_whole_context = true;
                }
                if let Some(triggers) = self.triggers.last_mut() {
                    triggers.extend(parsed);
                }
            }
            "CMD" | "LABEL" | "MAINTAINER" | "EXPOSE" | "ENTRYPOINT" | "VOLUME" | "USER"
            | "WORKDIR" | "STOPSIGNAL" | "HEALTHCHECK" | "SHELL" => {}
            _ => self.facts.claims_whole_context = true,
        }
        Ok(())
    }

    fn record_arg(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        let unresolved = || DockerfileProblem::ContextSourceUnresolved(format!("ARG {args}"));
        let words = arg_words(args).ok_or_else(unresolved)?;
        if words.is_empty() {
            return Err(unresolved());
        }
        for token in words {
            let (name, value) = match token.split_once('=') {
                Some((name, value)) => (
                    name,
                    arg_value(value).map_or(ArgValue::Unknown, ArgValue::Known),
                ),
                None if self.inherited_stage => (token, ArgValue::Unknown),
                None => (
                    token,
                    self.stage_args
                        .get(token)
                        .or_else(|| self.global_args.get(token))
                        .cloned()
                        .unwrap_or(ArgValue::Unset),
                ),
            };
            if !valid_name(name) {
                return Err(unresolved());
            }
            let value = if is_injected_arg(name) {
                ArgValue::Unknown
            } else {
                value
            };
            if self.triggers.is_empty() {
                self.global_args.insert(name.to_owned(), value);
            } else {
                self.stage_args.insert(name.to_owned(), value);
            }
        }
        Ok(())
    }

    fn from(&mut self, args: &str) -> Result<(), DockerfileProblem> {
        let mut tokens = args.split_whitespace().filter(|t| !t.starts_with("--"));
        let Some(raw) = tokens.next() else {
            return Ok(());
        };
        let reference = substitute(raw, &self.global_args)
            .ok_or_else(|| DockerfileProblem::ImageBaseUnresolved(raw.to_owned()))?;
        self.stage_args.clear();
        let local = self
            .stages
            .get(&reference.to_ascii_lowercase())
            .copied()
            .or_else(|| {
                reference
                    .parse::<usize>()
                    .ok()
                    .filter(|&index| index < self.triggers.len())
            });
        self.inherited_stage = local.is_some();
        self.unknown_environment = reference != "scratch";
        if reference != "scratch" && local.is_none() {
            require_pinned(&reference)?;
            self.facts.external_images.insert(reference);
            self.facts.claims_whole_context = true;
        }
        if let Some(index) = local {
            for (keyword, args) in self.triggers[index].clone() {
                match keyword.as_str() {
                    "COPY" | "ADD" | "RUN" | "ARG" | "ENV" => {
                        self.instruction(&keyword, &args)?;
                    }
                    _ => self.facts.claims_whole_context = true,
                }
            }
        }
        if tokens.next().is_some_and(|t| t.eq_ignore_ascii_case("AS"))
            && let Some(name) = tokens.next()
        {
            self.stages
                .insert(name.to_ascii_lowercase(), self.triggers.len());
        }
        self.triggers.push(Vec::new());
        Ok(())
    }

    fn copy(&mut self, keyword: &str, args: &str) -> Result<(), DockerfileProblem> {
        let mut from_other = false;
        let mut args = args.trim_start();
        while args.starts_with("--") {
            let (option, remainder) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
            if let Some(source) = option.strip_prefix("--from=") {
                from_other = true;
                self.check_from_source(source)?;
            }
            args = remainder.trim_start();
        }
        let json_form = args.trim_start().starts_with('[');
        if args.contains('\\') || (!json_form && args.contains(['"', '\''])) {
            return Err(DockerfileProblem::ContextSourceUnresolved(args.to_owned()));
        }
        let mut operands = split_operands(args)
            .ok_or_else(|| DockerfileProblem::ContextSourceUnresolved(args.to_owned()))?;
        let missing_destination = operands.len() < 2;
        if !missing_destination {
            operands.pop();
        }
        for source in operands {
            if !json_form && source.starts_with("<<") {
                continue;
            }
            if source.contains(['\\', '\'', '"']) {
                return Err(DockerfileProblem::ContextSourceUnresolved(source));
            }
            check_parent_components(&source)?;
            if keyword == "ADD" && is_remote(&source) {
                return Err(DockerfileProblem::RemoteAdd(source));
            }
            if self.unknown_environment && source.contains('$') {
                if keyword == "ADD" {
                    return Err(DockerfileProblem::RemoteAdd(source));
                }
                self.facts.claims_whole_context |= !from_other;
                continue;
            }
            let Some(source) = substitute(&source, &self.stage_args) else {
                if keyword == "ADD" {
                    return Err(DockerfileProblem::RemoteAdd(source));
                }
                self.facts.claims_whole_context |= !from_other;
                continue;
            };
            check_parent_components(&source)?;
            if !json_form && source.chars().any(char::is_whitespace) {
                return Err(DockerfileProblem::ContextSourceUnresolved(source));
            }
            if keyword == "ADD" && is_remote(&source) {
                return Err(DockerfileProblem::RemoteAdd(source));
            }
            if !from_other {
                self.facts.context_sources.insert(normalize_source(&source));
            }
        }
        self.facts.claims_whole_context |= missing_destination;
        Ok(())
    }

    fn check_from_source(&mut self, source: &str) -> Result<(), DockerfileProblem> {
        let is_stage = source.bytes().all(|b| b.is_ascii_digit())
            || self.stages.contains_key(&source.to_ascii_lowercase());
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

fn check_parent_components(source: &str) -> Result<(), DockerfileProblem> {
    if source.split('/').any(|part| part == "..") {
        return Err(DockerfileProblem::ContextEscape(source.to_owned()));
    }
    Ok(())
}

fn normalize_source(source: &str) -> String {
    let parts = source
        .split('/')
        .filter(|part| !matches!(*part, "" | "."))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
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
